//! Deferred common-owner composition fixtures; no transport or output runs here.
use crate::{
    local_players::PlayerId,
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_group_rooms::{GroupRoomPhase, GroupRoomPolicy, GroupRoomRegistry},
    multiplayer_protocol::{OutboundFrame, Progress},
    multiplayer_room_clock::RoomClockExchange,
    multiplayer_room_play::RoomPlayClient,
    multiplayer_room_progress::RoomProgressRelay,
    multiplayer_room_start::RoomStartCoordinator,
    multiplayer_room_wire::{RoomMessage, decode_message, encode_message},
    multiplayer_rooms::ParticipantId,
    multiplayer_start::{StartMessage, StartPolicy},
};

fn policy() -> StartPolicy {
    StartPolicy {
        lead_ns: 10_000,
        min_remaining_ns: 1_000,
        max_age_ns: 100_000,
        max_uncertainty_ns: 100,
        max_release_lateness_ns: 25,
    }
}

fn wire(frame: &OutboundFrame) -> RoomMessage {
    let message = decode_message(&frame.bytes).unwrap();
    assert_eq!(encode_message(&message).unwrap(), frame.bytes);
    message
}

fn deliver(client: &mut RoomPlayClient, message: RoomMessage, now: i64) {
    let bytes = encode_message(&message).unwrap();
    client
        .receive(decode_message(&bytes).unwrap(), now)
        .unwrap();
}

fn snapshot(registry: &GroupRoomRegistry) -> RoomMessage {
    let room = registry.room("room").unwrap();
    RoomMessage::Snapshot {
        members: room.members.to_vec(),
        phase: room.phase,
        deadline_ns: room.deadline_ns,
    }
}

struct Cohort {
    registry: GroupRoomRegistry,
    clients: Vec<RoomPlayClient>,
    ids: Vec<ParticipantId>,
}

fn collecting(count: usize) -> Cohort {
    let mut cohort = Cohort {
        registry: GroupRoomRegistry::new(GroupRoomPolicy::new(1, count, 8, 1_000).unwrap()),
        clients: Vec::new(),
        ids: Vec::new(),
    };
    for index in 0..count {
        let players = (0..=index % 4)
            .map(|slot| PlayerId(u32::MAX - slot as u32))
            .collect::<Vec<_>>();
        let mut client = RoomPlayClient::new(
            b"room-play\0identity",
            &players,
            policy(),
            index as i64 * 100,
        )
        .unwrap();
        let join = client.poll_write(0).unwrap().unwrap();
        assert_eq!(join.id, 1);
        let RoomMessage::Join {
            identity,
            players: actual_players,
        } = wire(&join)
        else {
            panic!("actual Join required")
        };
        assert_eq!(actual_players, players);
        let ticket = cohort
            .registry
            .join("room", &identity, &actual_players, 0)
            .unwrap();
        client.written(join.id, 0).unwrap();
        deliver(
            &mut client,
            RoomMessage::Admitted {
                participant: ticket.id,
            },
            0,
        );
        cohort.ids.push(ticket.id);
        cohort.clients.push(client);
        let actual = snapshot(&cohort.registry);
        for client in &mut cohort.clients {
            deliver(client, actual.clone(), 0);
        }
    }
    cohort
}

fn prepared(count: usize) -> Cohort {
    let mut cohort = collecting(count);
    cohort.clients[0].request_seal().unwrap();
    let seal = cohort.clients[0].poll_write(1).unwrap().unwrap();
    assert_eq!(wire(&seal), RoomMessage::Seal);
    cohort.clients[0].written(seal.id, 1).unwrap();
    cohort.registry.seal(cohort.ids[0], 1).unwrap();
    let frozen = snapshot(&cohort.registry);
    for client in &mut cohort.clients {
        deliver(client, frozen.clone(), 1);
    }
    for index in 0..count {
        cohort.clients[index].request_ready().unwrap();
        let ready = cohort.clients[index].poll_write(2).unwrap().unwrap();
        assert_eq!(wire(&ready), RoomMessage::Ready);
        cohort.clients[index].written(ready.id, 2).unwrap();
        cohort.registry.ready(cohort.ids[index], 2).unwrap();
        let actual = snapshot(&cohort.registry);
        for client in &mut cohort.clients {
            deliver(client, actual.clone(), 2);
        }
    }
    assert_eq!(
        cohort.registry.room("room").unwrap().phase,
        GroupRoomPhase::Prepared
    );
    cohort
}

fn offset(index: usize) -> i64 {
    if index % 2 == 0 { 400 } else { -400 }
}

fn clock_round(
    client: &mut RoomPlayClient,
    server: &mut RoomClockExchange,
    sequence: u64,
    at: i64,
    shift: i64,
) {
    let server_ping = server.next(at).unwrap().unwrap();
    let client_ping = client.poll_write(at + shift).unwrap().unwrap();
    assert_eq!(
        wire(&server_ping),
        RoomMessage::ClockPing {
            sequence,
            sent_ns: at
        }
    );
    assert_eq!(
        wire(&client_ping),
        RoomMessage::ClockPing {
            sequence,
            sent_ns: at + shift
        }
    );
    server.written(server_ping.id, at + 2).unwrap();
    client.written(client_ping.id, at + shift + 3).unwrap();
    server.receive(&wire(&client_ping), at + 7).unwrap();
    deliver(client, wire(&server_ping), at + shift + 11);
    let server_pong = server.next(at + 13).unwrap().unwrap();
    let client_pong = client.poll_write(at + shift + 17).unwrap().unwrap();
    assert_eq!(
        wire(&client_pong),
        RoomMessage::ClockPong {
            sequence,
            sent_ns: at,
            received_ns: at + shift + 11,
            replied_ns: at + shift + 17,
        }
    );
    server.written(server_pong.id, at + 14).unwrap();
    client.written(client_pong.id, at + shift + 18).unwrap();
    server.receive(&wire(&client_pong), at + 23).unwrap();
    deliver(client, wire(&server_pong), at + shift + 29);
    assert_eq!(client.take_schedule(), None);
}

fn committed(count: usize) -> (Cohort, i64) {
    committed_during_accept(count, |_, _, _, _| {})
}

fn committed_during_accept(
    count: usize,
    mut on_pending_commit: impl FnMut(
        &mut RoomPlayClient,
        crate::multiplayer_group_rooms::GroupRoomSnapshot<'_>,
        ParticipantId,
        i64,
    ),
) -> (Cohort, i64) {
    let mut cohort = prepared(count);
    let room = cohort.registry.room("room").unwrap();
    let mut servers = cohort
        .ids
        .iter()
        .map(|id| RoomClockExchange::new(room, *id).unwrap())
        .collect::<Vec<_>>();
    let mut coordinator = RoomStartCoordinator::new(room, policy()).unwrap();
    for index in 0..count {
        for sequence in 1..=8 {
            clock_round(
                &mut cohort.clients[index],
                &mut servers[index],
                sequence,
                10_000 + (sequence as i64 - 1) * 100,
                offset(index),
            );
        }
        let estimate = servers[index].estimate().unwrap();
        assert_eq!(
            (estimate.lower_ns(), estimate.upper_ns()),
            (
                i128::from(offset(index) - 6),
                i128::from(offset(index) + 11)
            )
        );
        coordinator
            .prepare(cohort.ids[index], estimate, 10_800)
            .unwrap();
    }
    for index in 0..count {
        let ready = coordinator
            .next(cohort.ids[index], 10_800)
            .unwrap()
            .unwrap();
        assert_eq!(ready, StartMessage::ClockReady(0));
        coordinator
            .written(cohort.ids[index], ready, 10_800)
            .unwrap();
        deliver(
            &mut cohort.clients[index],
            RoomMessage::Start(ready),
            10_800 + offset(index),
        );
    }
    for index in 0..count {
        let client = &mut cohort.clients[index];
        let ready = client.poll_write(10_801 + offset(index)).unwrap().unwrap();
        assert_eq!(ready.id, if index == 0 { 20 } else { 19 });
        assert_eq!(
            wire(&ready),
            RoomMessage::Start(StartMessage::ClockReady(index as i64 * 100))
        );
        assert!(client.poll_write(10_801 + offset(index)).unwrap().is_none());
        assert_eq!(client.take_schedule(), None);
        client.written(ready.id, 10_802 + offset(index)).unwrap();
        let RoomMessage::Start(ready) = wire(&ready) else {
            panic!("actual readiness required")
        };
        coordinator
            .receive(cohort.ids[index], ready, 10_803)
            .unwrap();
    }
    assert_eq!(coordinator.song_target_ns(), None);
    let target = 11_000 + 10_000 + (count as i64 - 1) * 100 + 17;
    for index in 0..count {
        let proposal = coordinator
            .next(cohort.ids[index], 11_000 + index as i64)
            .unwrap()
            .unwrap();
        assert_eq!(proposal, StartMessage::Propose(target));
        coordinator
            .written(cohort.ids[index], proposal, 11_000 + index as i64)
            .unwrap();
        deliver(
            &mut cohort.clients[index],
            RoomMessage::Start(proposal),
            11_080 + offset(index),
        );
    }
    assert_eq!(coordinator.song_target_ns(), Some(target));
    let mut delayed_accept = None;
    for index in 0..count {
        let client = &mut cohort.clients[index];
        let accept = client.poll_write(11_081 + offset(index)).unwrap().unwrap();
        assert_eq!(accept.id, if index == 0 { 21 } else { 20 });
        assert_eq!(
            wire(&accept),
            RoomMessage::Start(StartMessage::Accept(target))
        );
        let before = client.clone();
        assert!(
            client
                .receive(
                    RoomMessage::Start(StartMessage::Commit(target + 1)),
                    11_082 + offset(index)
                )
                .is_err()
        );
        assert_eq!(*client, before);
        assert!(client.written(1, 11_900 + offset(index)).is_err());
        assert_eq!(*client, before);
        if index + 1 == count {
            // The frame reaches the server, while its real local completion
            // notification remains queued behind later received controls.
            delayed_accept = Some((accept.id, 11_082 + offset(index)));
        } else {
            client.written(accept.id, 11_082 + offset(index)).unwrap();
        }
        let before = client.clone();
        assert!(
            client
                .receive(
                    RoomMessage::Start(StartMessage::Commit(target + 1)),
                    11_900 + offset(index)
                )
                .is_err()
        );
        assert_eq!(*client, before);
        assert_eq!(client.take_schedule(), None);
        let RoomMessage::Start(accept) = wire(&accept) else {
            panic!("actual Accept required")
        };
        coordinator
            .receive(cohort.ids[index], accept, 11_090)
            .unwrap();
        if index + 1 < count {
            assert!(coordinator.next(cohort.ids[0], 11_090).unwrap().is_none());
        }
        assert!(!coordinator.committed());
    }
    for index in 0..count {
        let commit = coordinator
            .next(cohort.ids[index], 11_100 + index as i64)
            .unwrap()
            .unwrap();
        assert_eq!(commit, StartMessage::Commit(target));
        assert!(!coordinator.committed());
        coordinator
            .written(cohort.ids[index], commit, 11_101 + index as i64)
            .unwrap();
        if index + 1 == count {
            let client = &mut cohort.clients[index];
            client
                .receive_at(
                    RoomMessage::Start(commit),
                    11_200 + offset(index),
                    11_250 + offset(index),
                )
                .unwrap();
            assert_eq!(client.take_schedule(), None);
            let pending = client.clone();
            assert!(
                client
                    .receive_at(
                        RoomMessage::Start(commit),
                        11_201 + offset(index),
                        11_251 + offset(index)
                    )
                    .is_err()
            );
            assert_eq!(*client, pending);
            assert!(client.poll_write(11_251 + offset(index)).unwrap().is_none());
            assert!(coordinator.committed());
            on_pending_commit(client, room, cohort.ids[index], 11_200 + offset(index));
            let (write_id, completed) = delayed_accept.unwrap();
            client
                .written_at(write_id, completed, 11_252 + offset(index))
                .unwrap();
        } else {
            deliver(
                &mut cohort.clients[index],
                RoomMessage::Start(commit),
                11_200 + offset(index),
            );
        }
    }
    assert!(coordinator.committed());
    (cohort, target)
}

#[test]
fn constructor_owns_original_setup_and_refuses_invalid_policy_preroll_and_rosters() {
    let mut identity = b"original identity".to_vec();
    let mut players = vec![PlayerId(u32::MAX), PlayerId(1)];
    let mut client = RoomPlayClient::new(&identity, &players, policy(), 777).unwrap();
    identity.fill(0);
    players.clear();
    assert_eq!(client.participant(), None);
    assert!(client.room().is_none());
    assert_eq!(client.take_schedule(), None);
    let join = client.poll_write(0).unwrap().unwrap();
    assert_eq!(
        wire(&join),
        RoomMessage::Join {
            identity: b"original identity".to_vec(),
            players: vec![PlayerId(u32::MAX), PlayerId(1)],
        }
    );
    for invalid in [
        Vec::new(),
        vec![PlayerId(0)],
        vec![PlayerId(1), PlayerId(1)],
        (1..=65).map(PlayerId).collect::<Vec<_>>(),
    ] {
        assert!(RoomPlayClient::new(b"id", &invalid, policy(), 0).is_err());
    }
    for invalid in [Vec::new(), vec![0; 65_537]] {
        assert!(RoomPlayClient::new(&invalid, &[PlayerId(1)], policy(), 0).is_err());
    }
    for invalid in [
        StartPolicy {
            min_remaining_ns: 0,
            ..policy()
        },
        StartPolicy {
            lead_ns: 1_000,
            ..policy()
        },
        StartPolicy {
            max_age_ns: u64::MAX,
            ..policy()
        },
    ] {
        assert!(RoomPlayClient::new(b"id", &[PlayerId(1)], invalid, 0).is_err());
    }
    assert!(RoomPlayClient::new(b"id", &[PlayerId(1)], policy(), -1).is_err());
}

#[test]
fn registry_admission_and_prepared_membership_require_actual_own_request_write_receipts() {
    let mut cohort = collecting(2);
    assert!(cohort.clients[1].request_seal().is_err());
    assert!(cohort.clients[0].request_ready().is_err());
    cohort.clients[0].request_seal().unwrap();
    let seal = cohort.clients[0].poll_write(1).unwrap().unwrap();
    assert_eq!(seal.id, 2);
    cohort.registry.seal(cohort.ids[0], 1).unwrap();
    let frozen = snapshot(&cohort.registry);
    let before = cohort.clients[0].clone();
    assert!(cohort.clients[0].receive(frozen.clone(), 500).is_err());
    assert_eq!(cohort.clients[0], before);
    cohort.clients[0].written(seal.id, 1).unwrap();
    for client in &mut cohort.clients {
        deliver(client, frozen.clone(), 1);
    }
    for index in 0..2 {
        cohort.clients[index].request_ready().unwrap();
        let ready = cohort.clients[index].poll_write(2).unwrap().unwrap();
        assert_eq!(ready.id, if index == 0 { 3 } else { 2 });
        assert_eq!(wire(&ready), RoomMessage::Ready);
        assert!(cohort.clients[index].poll_write(2).unwrap().is_none());
        cohort.registry.ready(cohort.ids[index], 2).unwrap();
        let actual = snapshot(&cohort.registry);
        let before = cohort.clients[index].clone();
        assert!(cohort.clients[index].receive(actual.clone(), 500).is_err());
        assert_eq!(cohort.clients[index], before);
        assert!(
            cohort.clients[index]
                .receive(
                    RoomMessage::ClockPing {
                        sequence: 1,
                        sent_ns: 2
                    },
                    500
                )
                .is_err()
        );
        assert_eq!(cohort.clients[index], before);
        cohort.clients[index].written(ready.id, 2).unwrap();
        for client in &mut cohort.clients {
            deliver(client, actual.clone(), 2);
        }
    }
    for (index, client) in cohort.clients.iter_mut().enumerate() {
        assert_eq!(client.participant(), Some(cohort.ids[index]));
        assert_eq!(client.room().unwrap().phase, GroupRoomPhase::Prepared);
        let ping = client.poll_write(3).unwrap().unwrap();
        assert_eq!(ping.id, if index == 0 { 4 } else { 3 });
        assert_eq!(
            wire(&ping),
            RoomMessage::ClockPing {
                sequence: 1,
                sent_ns: 3
            }
        );
        assert_eq!(client.take_schedule(), None);
    }
}

#[test]
fn actual_admission_clocks_and_coordinator_translate_one_common_target_once_per_client() {
    for count in [2usize, 3, 4, 64] {
        let (mut cohort, target) = committed(count);
        for (index, client) in cohort.clients.iter_mut().enumerate() {
            let schedule = client.take_schedule().unwrap();
            // Client's measured server-minus-client interval is
            // [-offset-16, -offset+7]; translation retains its midpoint/width.
            assert_eq!(schedule.song_target_ns, target + offset(index) + 4);
            assert_eq!(
                schedule.target_ns,
                target + offset(index) + 4 - index as i64 * 100
            );
            assert_eq!(schedule.uncertainty_ns, 23);
            assert_eq!(client.take_schedule(), None);
            assert!(client.poll_write(11_300 + offset(index)).unwrap().is_none());
            assert!(!client.leave_written());
        }
    }
}

#[test]
fn real_peer_clock_ready_can_arrive_while_final_client_pong_write_is_still_pending() {
    let mut cohort = prepared(2);
    let room = cohort.registry.room("room").unwrap();
    let mut server = RoomClockExchange::new(room, cohort.ids[0]).unwrap();
    let mut coordinator = RoomStartCoordinator::new(room, policy()).unwrap();
    let client = &mut cohort.clients[0];
    for sequence in 1..=7 {
        clock_round(
            client,
            &mut server,
            sequence,
            10_000 + (sequence as i64 - 1) * 100,
            400,
        );
    }
    let server_ping = server.next(10_700).unwrap().unwrap();
    let client_ping = client.poll_write(11_100).unwrap().unwrap();
    server.written(server_ping.id, 10_702).unwrap();
    server.receive(&wire(&client_ping), 10_707).unwrap();
    deliver(client, wire(&server_ping), 11_111);
    let server_pong = server.next(10_713).unwrap().unwrap();
    server.written(server_pong.id, 10_714).unwrap();
    deliver(client, wire(&server_pong), 11_129);
    assert!(client.poll_write(11_129).unwrap().is_none());
    client.written(client_ping.id, 11_130).unwrap();
    let client_pong = client.poll_write(11_131).unwrap().unwrap();
    assert_eq!(
        wire(&client_pong),
        RoomMessage::ClockPong {
            sequence: 8,
            sent_ns: 10_700,
            received_ns: 11_111,
            replied_ns: 11_131,
        }
    );
    server.receive(&wire(&client_pong), 10_740).unwrap();
    coordinator
        .prepare(cohort.ids[0], server.estimate().unwrap(), 10_740)
        .unwrap();
    let ready = coordinator.next(cohort.ids[0], 10_741).unwrap().unwrap();
    assert_eq!(ready, StartMessage::ClockReady(0));
    coordinator.written(cohort.ids[0], ready, 10_742).unwrap();
    deliver(client, RoomMessage::Start(ready), 11_145);
    assert!(client.poll_write(11_146).unwrap().is_none());
    let before = client.clone();
    assert!(
        client
            .receive(RoomMessage::Start(StartMessage::Propose(30_000)), 11_149)
            .is_err()
    );
    assert_eq!(*client, before);
    client.written(client_pong.id, 11_150).unwrap();
    let local_ready = client.poll_write(11_151).unwrap().unwrap();
    assert_eq!(local_ready.id, 20);
    assert_eq!(
        wire(&local_ready),
        RoomMessage::Start(StartMessage::ClockReady(0))
    );
    assert_eq!(client.take_schedule(), None);
    let before = client.clone();
    assert!(
        client
            .receive(RoomMessage::Start(StartMessage::Propose(30_000)), 11_152)
            .is_err()
    );
    assert_eq!(*client, before);
    client.written(local_ready.id, 11_152).unwrap();
    coordinator
        .receive(cohort.ids[0], StartMessage::ClockReady(0), 10_753)
        .unwrap();
    assert!(coordinator.next(cohort.ids[0], 10_754).unwrap().is_none());
    assert_eq!(coordinator.song_target_ns(), None);
    assert_eq!(client.take_schedule(), None);
}

#[test]
fn invalid_controls_receipts_and_times_leave_admission_and_clock_state_atomic() {
    let mut initial = RoomPlayClient::new(b"id", &[PlayerId(1)], policy(), 0).unwrap();
    for control in [
        RoomMessage::ClockPing {
            sequence: 1,
            sent_ns: 0,
        },
        RoomMessage::Start(StartMessage::ClockReady(0)),
        RoomMessage::Admitted {
            participant: ParticipantId(1),
        },
    ] {
        let before = initial.clone();
        assert!(initial.receive(control, 50).is_err());
        assert_eq!(initial, before);
    }
    let join = initial.poll_write(0).unwrap().unwrap();
    let before = initial.clone();
    assert!(
        initial
            .receive(
                RoomMessage::Admitted {
                    participant: ParticipantId(1)
                },
                50
            )
            .is_err()
    );
    assert_eq!(initial, before);
    initial.written(join.id, 0).unwrap();

    let mut cohort = prepared(2);
    let client = &mut cohort.clients[0];
    let ping = client.poll_write(100).unwrap().unwrap();
    for control in [
        RoomMessage::Ready,
        RoomMessage::Admitted {
            participant: ParticipantId(u64::MAX),
        },
        RoomMessage::ClockPing {
            sequence: 0,
            sent_ns: 100,
        },
        RoomMessage::ClockPing {
            sequence: 2,
            sent_ns: 100,
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
            received_ns: 111,
            replied_ns: 110,
        },
        RoomMessage::Start(StartMessage::Propose(30_000)),
        RoomMessage::Start(StartMessage::Commit(30_000)),
    ] {
        let before = client.clone();
        assert!(client.receive(control, 200).is_err());
        assert_eq!(*client, before);
    }
    for now in [-1, 99] {
        let before = client.clone();
        assert!(client.poll_write(now).is_err());
        assert_eq!(*client, before);
        assert!(client.written(ping.id, now).is_err());
        assert_eq!(*client, before);
        assert!(
            client
                .receive(
                    RoomMessage::ClockPing {
                        sequence: 1,
                        sent_ns: 0
                    },
                    now
                )
                .is_err()
        );
        assert_eq!(*client, before);
    }
    for id in [0, 1, u64::MAX] {
        let before = client.clone();
        assert!(client.written(id, 200).is_err());
        assert_eq!(*client, before);
    }
    let before = client.clone();
    assert!(client.request_ready().is_err());
    assert!(client.request_seal().is_err());
    assert!(client.request_leave().is_err());
    assert_eq!(*client, before);
    client.written(ping.id, 100).unwrap();
    deliver(
        client,
        RoomMessage::ClockPong {
            sequence: 1,
            sent_ns: 100,
            received_ns: 105,
            replied_ns: 110,
        },
        120,
    );
    let before = client.clone();
    assert!(
        client
            .receive(
                RoomMessage::ClockPong {
                    sequence: 1,
                    sent_ns: 100,
                    received_ns: 105,
                    replied_ns: 110,
                },
                200
            )
            .is_err()
    );
    assert_eq!(*client, before);
    deliver(client, RoomMessage::Start(StartMessage::ClockReady(0)), 120);
    let before = client.clone();
    assert!(
        client
            .receive(RoomMessage::Start(StartMessage::ClockReady(0)), 200)
            .is_err()
    );
    assert_eq!(*client, before);
    assert_eq!(client.take_schedule(), None);

    let mut queued = prepared(2);
    let client = &mut queued.clients[0];
    let ping = client.poll_write(100).unwrap().unwrap();
    assert!(client.poll_write(200).unwrap().is_none());
    client.written_at(ping.id, 101, 201).unwrap();
    client
        .receive_at(
            RoomMessage::ClockPong {
                sequence: 1,
                sent_ns: 100,
                received_ns: 105,
                replied_ns: 110,
            },
            120,
            202,
        )
        .unwrap();
    let peer_ping = RoomMessage::ClockPing {
        sequence: 1,
        sent_ns: 5,
    };
    let before = client.clone();
    assert!(client.receive_at(peer_ping.clone(), 119, 203).is_err());
    assert_eq!(*client, before);
    assert!(client.receive_at(peer_ping.clone(), 204, 203).is_err());
    assert_eq!(*client, before);
    client.receive_at(peer_ping, 121, 203).unwrap();
    let pong = client.poll_write(204).unwrap().unwrap();
    assert_eq!(
        wire(&pong),
        RoomMessage::ClockPong {
            sequence: 1,
            sent_ns: 5,
            received_ns: 121,
            replied_ns: 204,
        }
    );
}

#[test]
fn leave_and_stop_fence_clock_start_and_unconsumed_schedules_without_fabricating_writes() {
    let mut cohort = prepared(2);
    let client = &mut cohort.clients[0];
    client.request_leave().unwrap();
    assert_eq!(client.take_schedule(), None);
    let leave = client.poll_write(3).unwrap().unwrap();
    assert_eq!(leave.id, 4);
    assert_eq!(wire(&leave), RoomMessage::Leave);
    assert!(!client.leave_written());
    assert!(client.poll_write(4).unwrap().is_none());
    let before = client.clone();
    assert!(
        client
            .receive(
                RoomMessage::ClockPing {
                    sequence: 1,
                    sent_ns: 0
                },
                5
            )
            .is_err()
    );
    assert_eq!(*client, before);
    assert!(client.written(leave.id + 1, 5).is_err());
    assert_eq!(*client, before);
    client.written(leave.id, 4).unwrap();
    assert!(client.leave_written());
    assert!(client.poll_write(4).unwrap().is_none());
    assert_eq!(client.take_schedule(), None);

    let (mut committed, _) = committed(2);
    committed.clients[0].request_leave().unwrap();
    assert_eq!(committed.clients[0].take_schedule(), None);
    let leave = committed.clients[0].poll_write(11_700).unwrap().unwrap();
    assert_eq!(wire(&leave), RoomMessage::Leave);
    assert_eq!(leave.id, 22);
    assert!(!committed.clients[0].leave_written());

    let mut unstarted = RoomPlayClient::new(b"id", &[PlayerId(1)], policy(), 0).unwrap();
    let pending = unstarted.poll_write(0).unwrap().unwrap();
    for (client, now, receipt) in [
        (&mut unstarted, 0, pending.id),
        (&mut committed.clients[1], 11_700, 20),
    ] {
        let participant = client.participant();
        client.stop();
        assert_eq!(client.take_schedule(), None);
        let stopped = client.clone();
        client.stop();
        assert!(client.poll_write(now).is_err());
        assert!(client.written(receipt, now).is_err());
        assert!(
            client
                .receive(RoomMessage::Start(StartMessage::Commit(30_000)), now)
                .is_err()
        );
        assert!(client.request_seal().is_err());
        assert!(client.request_ready().is_err());
        assert!(client.request_leave().is_err());
        assert_eq!(*client, stopped);
        assert_eq!(client.participant(), participant);
    }
}

fn progress_rows(players: &[PlayerId], hits: u64) -> Vec<MemberProgress> {
    players
        .iter()
        .map(|&player| MemberProgress {
            player,
            progress: Progress {
                song_ns: 604_800_000_000_000 + hits as i64,
                hits,
                misses: 0,
                combo: hits,
                max_combo: hits,
            },
        })
        .collect()
}

#[test]
fn actual_committed_clients_and_relay_keep_global_write_ids_and_separate_every_final_receipt() {
    for count in [2usize, 3, 4] {
        let (mut cohort, _) = committed(count);
        let snapshot = cohort.registry.room("room").unwrap();
        let mut relay = RoomProgressRelay::new(snapshot).unwrap();
        relay.activate().unwrap();
        let mut outer_ids = Vec::new();
        let mut delayed_upload = None;
        for index in 0..count {
            let client = &mut cohort.clients[index];
            client
                .publish_progress(&progress_rows(&snapshot.members[index].players, 1), false)
                .unwrap();
            client
                .publish_progress(&progress_rows(&snapshot.members[index].players, 2), true)
                .unwrap();
            let at = 12_000 + offset(index);
            let upload = client.poll_write(at).unwrap().unwrap();
            assert_eq!(upload.id, if index == 0 { 22 } else { 21 });
            let expected = GroupPrefix {
                sequence: 1,
                final_prefix: true,
                members: progress_rows(&snapshot.members[index].players, 2),
            };
            assert_eq!(wire(&upload), RoomMessage::Progress(expected));
            relay.receive(cohort.ids[index], &wire(&upload)).unwrap();
            outer_ids.push(upload.id);
            if index == 0 {
                delayed_upload = Some((upload.id, at + 1));
            } else {
                client.written_at(upload.id, at + 1, at + 2).unwrap();
            }
            assert!(!client.progress_complete());
        }
        let mut received_early_aggregate = false;
        let mut final_relay_write = None;
        let mut aggregates_seen = 0;
        for turn in 0..count * 6 {
            let at = 13_000 + turn as i64 * 100;
            for index in 0..count {
                let own = cohort.ids[index];
                if let Some(frame) = relay.poll_write(own).unwrap() {
                    let message = decode_message(&frame.bytes).unwrap();
                    let aggregate = matches!(message, RoomMessage::FinalAck { .. });
                    let client = &mut cohort.clients[index];
                    client
                        .receive_at(message, at + offset(index) + 10, at + offset(index) + 20)
                        .unwrap();
                    if aggregate && index == 0 && delayed_upload.is_some() {
                        assert!(!client.local_final_written());
                        assert!(!client.local_final_acknowledged());
                        assert!(
                            client
                                .poll_write(at + offset(index) + 21)
                                .unwrap()
                                .is_none()
                        );
                        let (id, completed) = delayed_upload.take().unwrap();
                        client
                            .written_at(id, completed, at + offset(index) + 22)
                            .unwrap();
                        assert!(client.local_final_written() && client.local_final_acknowledged());
                        received_early_aggregate = true;
                    }
                    // Hold one real server aggregate write to distinguish all
                    // local receipt boundaries from whole-relay completion.
                    if aggregate {
                        aggregates_seen += 1;
                    }
                    if aggregate && aggregates_seen == count {
                        final_relay_write = Some((own, frame.id));
                    } else {
                        relay.written(own, frame.id).unwrap();
                    }
                }
            }
            for index in 0..count {
                let client = &mut cohort.clients[index];
                if let Some(frame) = client.poll_write(at + offset(index) + 30).unwrap() {
                    assert_eq!(frame.id, outer_ids[index] + 1);
                    outer_ids[index] = frame.id;
                    let RoomMessage::FinalAck {
                        participant,
                        sequence,
                    } = wire(&frame)
                    else {
                        panic!("real recipient ACK required")
                    };
                    assert_ne!(participant, cohort.ids[index]);
                    assert_eq!(sequence, 1);
                    assert!(!client.peer_final_ack_written(participant));
                    relay.receive(cohort.ids[index], &wire(&frame)).unwrap();
                    client
                        .written_at(frame.id, at + offset(index) + 31, at + offset(index) + 32)
                        .unwrap();
                    assert!(client.peer_final_ack_written(participant));
                }
            }
            if cohort.clients.iter().all(RoomPlayClient::progress_complete) {
                break;
            }
        }
        assert!(received_early_aggregate);
        assert!(delayed_upload.is_none());
        assert!(cohort.clients.iter().all(RoomPlayClient::progress_complete));
        assert!(
            !relay.complete(),
            "local client completion is not authority to close the whole room"
        );
        let (recipient, id) = final_relay_write.unwrap();
        relay.written(recipient, id).unwrap();
        assert!(relay.complete());
        for (index, client) in cohort.clients.iter_mut().enumerate() {
            for remote in cohort
                .ids
                .iter()
                .copied()
                .filter(|id| *id != cohort.ids[index])
            {
                assert!(client.peer_progress(remote).unwrap().final_prefix);
                assert!(client.peer_final_ack_written(remote));
            }
            assert!(client.poll_write(20_000 + offset(index)).unwrap().is_none());
            assert!(!client.leave_written());
        }
    }
}

#[test]
fn matching_pending_commit_stages_real_relay_prefix_until_accept_write_then_leave_fences_progress()
{
    let mut uncommitted = prepared(2);
    let snapshot = uncommitted.registry.room("room").unwrap();
    let early = RoomMessage::PeerProgress {
        participant: uncommitted.ids[0],
        prefix: GroupPrefix {
            sequence: 1,
            final_prefix: true,
            members: progress_rows(&snapshot.members[0].players, 1),
        },
    };
    let before = uncommitted.clients[1].clone();
    assert!(uncommitted.clients[1].receive(early, 3).is_err());
    assert_eq!(uncommitted.clients[1], before);
    assert!(
        uncommitted.clients[1]
            .publish_progress(&progress_rows(&snapshot.members[1].players, 1), false)
            .is_err()
    );
    assert_eq!(uncommitted.clients[1], before);

    let mut staged_relay = None;
    let (mut cohort, _) = committed_during_accept(2, |client, snapshot, own, captured_commit| {
        let mut relay = RoomProgressRelay::new(snapshot).unwrap();
        relay.activate().unwrap();
        let source = &snapshot.members[0];
        let upload = RoomMessage::Progress(GroupPrefix {
            sequence: 1,
            final_prefix: true,
            members: progress_rows(&source.players, 1),
        });
        relay.receive(source.id, &upload).unwrap();
        let frame = relay.poll_write(own).unwrap().unwrap();
        let message = decode_message(&frame.bytes).unwrap();
        let before = client.clone();
        assert!(
            client
                .receive_at(message.clone(), captured_commit - 1, captured_commit + 51)
                .is_err()
        );
        assert_eq!(*client, before);
        client
            .receive_at(message, captured_commit + 1, captured_commit + 51)
            .unwrap();
        assert!(client.peer_progress(source.id).unwrap().final_prefix);
        assert!(!client.peer_final_ack_written(source.id));
        assert!(client.poll_write(captured_commit + 51).unwrap().is_none());
        assert!(
            client
                .publish_progress(&progress_rows(&snapshot.members[1].players, 1), true)
                .is_err()
        );
        assert_eq!(client.take_schedule(), None);
        relay.written(own, frame.id).unwrap();
        staged_relay = Some(relay);
    });
    let own = cohort.ids[1];
    let remote = cohort.ids[0];
    let client = &mut cohort.clients[1];
    assert!(client.take_schedule().is_some());
    let ack = client.poll_write(12_000).unwrap().unwrap();
    assert_eq!(ack.id, 21);
    assert_eq!(
        wire(&ack),
        RoomMessage::FinalAck {
            participant: remote,
            sequence: 1
        }
    );
    let mut relay = staged_relay.unwrap();
    relay.receive(own, &wire(&ack)).unwrap();
    assert!(relay.final_acknowledged(remote).unwrap());
    assert!(!client.peer_final_ack_written(remote));
    client.written_at(ack.id, 12_001, 12_002).unwrap();
    assert!(client.peer_final_ack_written(remote));
    let players = client.room().unwrap().members[1].players.clone();
    client
        .publish_progress(&progress_rows(&players, 1), false)
        .unwrap();
    client.request_leave().unwrap();
    let before = client.clone();
    assert!(
        client
            .publish_progress(&progress_rows(&players, 2), true)
            .is_err()
    );
    assert!(
        client
            .receive_at(
                RoomMessage::FinalAck {
                    participant: own,
                    sequence: 1
                },
                12_003,
                12_004
            )
            .is_err()
    );
    assert_eq!(*client, before);
    let leave = client.poll_write(12_004).unwrap().unwrap();
    assert_eq!((leave.id, wire(&leave)), (22, RoomMessage::Leave));
    assert!(!client.leave_written());
    client.written(leave.id, 12_005).unwrap();
    assert!(client.leave_written());
    assert!(!client.progress_complete());
    assert!(client.poll_write(12_006).unwrap().is_none());
    client.stop();
    assert!(
        client
            .publish_progress(&progress_rows(&players, 2), true)
            .is_err()
    );

    // Leave may fence an already admitted upload, but its original bytes still
    // own the outer write slot until the exact full-write receipt arrives.
    let own = cohort.ids[0];
    let client = &mut cohort.clients[0];
    let players = client.room().unwrap().members[0].players.clone();
    client
        .publish_progress(&progress_rows(&players, 7), true)
        .unwrap();
    let upload = client.poll_write(13_000).unwrap().unwrap();
    assert_eq!(upload.id, 22);
    assert_eq!(
        wire(&upload),
        RoomMessage::Progress(GroupPrefix {
            sequence: 1,
            final_prefix: true,
            members: progress_rows(&players, 7),
        })
    );
    client.request_leave().unwrap();
    assert!(!client.local_final_written());
    assert!(!client.local_final_acknowledged());
    assert!(!client.progress_complete());
    assert!(client.poll_write(13_001).unwrap().is_none());
    let fenced = client.clone();
    assert!(
        client
            .publish_progress(&progress_rows(&players, 8), true)
            .is_err()
    );
    assert!(
        client
            .receive_at(
                RoomMessage::FinalAck {
                    participant: own,
                    sequence: 1
                },
                13_001,
                13_002
            )
            .is_err()
    );
    assert!(client.written_at(upload.id + 1, 13_001, 13_002).is_err());
    assert_eq!(*client, fenced);
    client.written_at(upload.id, 13_001, 13_003).unwrap();
    assert!(
        !client.local_final_written(),
        "post-fence transport completion grants no progress credit"
    );
    assert!(!client.local_final_acknowledged());
    assert!(!client.progress_complete());
    let leave = client.poll_write(13_004).unwrap().unwrap();
    assert_eq!((leave.id, wire(&leave)), (23, RoomMessage::Leave));
    assert!(!client.leave_written());
    client.written_at(leave.id, 13_005, 13_006).unwrap();
    assert!(client.leave_written());
    assert!(client.poll_write(13_007).unwrap().is_none());
    assert!(!client.local_final_written());
    assert!(!client.progress_complete());
}
