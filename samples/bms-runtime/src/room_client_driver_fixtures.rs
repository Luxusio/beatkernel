//! Deferred split-operation fixtures using genuine common admission frames.
use super::{RoomClientDriver, RoomDrainError, RoomClientSetupError};
use crate::{
    room_final_wait::RoomFinalStep,
    room_frame_wait::{RoomFrameWaitError, RoomFrameWaitStep},
    room_setup_wait::{RoomSetupStep, RoomSetupError, RoomSetupPhase, RoomDeadlineError},
    local_players::PlayerId,
    multiplayer_group::{MemberProgress, encode_words},
    multiplayer_group_rooms::{GroupRoomPolicy, GroupRoomRegistry},
    multiplayer_protocol::Progress,
    multiplayer_room_clock::RoomClockExchange,
    multiplayer_room_play::RoomPlayError,
    multiplayer_room_progress::RoomProgressRelay,
    multiplayer_room_start::RoomStartCoordinator,
    multiplayer_room_wire::{RoomMessage, decode_message, encode_message},
    multiplayer_rooms::ParticipantId,
    multiplayer_start::{StartMessage, StartPolicy},
};

const IDENTITY: &[u8] = &[0, 91, 255, 17];
const PLAYERS: &[PlayerId] = &[PlayerId(u32::MAX), PlayerId(7)];

fn driver() -> RoomClientDriver {
    RoomClientDriver::new(IDENTITY, PLAYERS, StartPolicy::default(), 0).unwrap()
}
fn receive(
    driver: &mut RoomClientDriver,
    message: &RoomMessage,
    now: i64,
) -> Result<(), RoomPlayError> {
    let bytes = encode_message(message).unwrap();
    let mut offset = 0;
    while offset < bytes.len() {
        let count = driver.needed_bytes()?.min(bytes.len() - offset);
        assert!(count > 0);
        assert_eq!(
            driver.receive_bytes(&bytes[offset..offset + count], now, now)?,
            count
        );
        offset += count;
    }
    Ok(())
}
fn snapshot(registry: &GroupRoomRegistry) -> RoomMessage {
    let room = registry.room("driver").unwrap();
    RoomMessage::Snapshot {
        members: room.members.to_vec(),
        phase: room.phase,
        deadline_ns: room.deadline_ns,
    }
}
fn admitted(now: i64) -> (RoomClientDriver, GroupRoomRegistry, ParticipantId) {
    let mut driver = driver();
    let frame = driver.next_write(now).unwrap().expect("actual Join");
    let RoomMessage::Join { identity, players } = decode_message(&frame.bytes).unwrap() else {
        panic!("actual Join required")
    };
    assert_eq!(identity, IDENTITY);
    assert_eq!(players, PLAYERS);
    driver.written(frame.id, now, now).unwrap();
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 2, 16, 1_000).unwrap());
    let id = registry.join("driver", &identity, &players, 0).unwrap().id;
    receive(&mut driver, &RoomMessage::Admitted { participant: id }, now).unwrap();
    receive(&mut driver, &snapshot(&registry), now).unwrap();
    (driver, registry, id)
}
fn no_final_receipts(driver: &RoomClientDriver) {
    assert!(!driver.local_final_written());
    assert!(!driver.local_final_acknowledged());
    assert!(!driver.peer_final_ack_written(ParticipantId(u64::MAX)));
    assert!(!driver.progress_complete());
    assert!(!driver.drain_complete());
}

#[test]
fn construction_rejects_invalid_identity_roster_and_start_configuration() {
    for players in [
        vec![],
        vec![PlayerId(0)],
        vec![PlayerId(7), PlayerId(7)],
        (1..=65).map(PlayerId).collect(),
    ] {
        assert!(RoomClientDriver::new(IDENTITY, &players, StartPolicy::default(), 0).is_err());
    }
    for identity in [Vec::new(), vec![1; 65_537]] {
        assert!(RoomClientDriver::new(&identity, PLAYERS, StartPolicy::default(), 0).is_err());
    }
    assert!(RoomClientDriver::new(IDENTITY, PLAYERS, StartPolicy::default(), -1).is_err());
    let mut policy = StartPolicy::default();
    policy.min_remaining_ns = 0;
    assert!(RoomClientDriver::new(IDENTITY, PLAYERS, policy, 0).is_err());
    let valid = RoomClientDriver::new(
        IDENTITY,
        PLAYERS,
        StartPolicy::default(),
        604_800_000_000_000,
    )
    .unwrap();
    assert!(!valid.failed());
    assert_eq!(valid.revision(), 0);
}

#[test]
fn local_phase_refusals_remain_recoverable_but_malformed_progress_latches_first_failure() {
    let mut driver = driver();
    assert!(driver.request_seal().is_err());
    assert!(driver.request_ready().is_err());
    assert!(driver.request_leave().is_err());
    assert!(driver.request_drain().is_err());
    assert!(!driver.failed());
    assert!(driver.next_write(0).unwrap().is_some());
    let first = driver.publish_progress_words(&[1], false).unwrap_err();
    assert!(driver.failed());
    assert_eq!(driver.request_ready().unwrap_err(), first);
    assert_eq!(driver.needed_bytes().unwrap_err(), first);
    assert_eq!(driver.next_write(1).unwrap_err(), first);
    driver.close();
    driver.close();
    assert_eq!(driver.session_ref().unwrap_err(), first);
    no_final_receipts(&driver);
}

#[test]
fn fragmented_admission_advances_revision_only_after_complete_accepted_frames() {
    let mut driver = driver();
    let frame = driver.next_write(0).unwrap().unwrap();
    let RoomMessage::Join { identity, players } = decode_message(&frame.bytes).unwrap() else {
        panic!("Join required")
    };
    driver.written(frame.id, 0, 0).unwrap();
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 2, 16, 1_000).unwrap());
    let id = registry.join("driver", &identity, &players, 0).unwrap().id;
    for (message, previous) in [
        (RoomMessage::Admitted { participant: id }, 0),
        (snapshot(&registry), 1),
    ] {
        let bytes = encode_message(&message).unwrap();
        for (index, byte) in bytes.iter().enumerate() {
            assert!(driver.needed_bytes().unwrap() > 0);
            assert_eq!(
                driver
                    .receive_bytes(std::slice::from_ref(byte), 1, 1)
                    .unwrap(),
                1
            );
            let complete = index + 1 == bytes.len();
            assert_eq!(driver.revision(), previous + u64::from(complete));
            assert_eq!(driver.frame_pending(), !complete);
        }
    }
    assert_eq!(driver.participant_id(), id.0);
    assert!(driver.has_snapshot());
    assert_eq!(
        driver.session_ref().unwrap().room().unwrap().members[0].players,
        PLAYERS
    );
    assert!(driver.pending_peer().unwrap().is_none());
    driver.consume_peer_progress();
    assert!(driver.pending_peer().unwrap().is_none());
    assert!(driver.take_start().unwrap().is_none());
    no_final_receipts(&driver);
}

#[test]
fn malformed_prefix_and_observation_refusals_preserve_accepted_metadata_revision() {
    for failure in 0..3 {
        let (mut driver, _, id) = admitted(10);
        let before = driver.revision();
        let needed = driver.needed_bytes().unwrap();
        let first = match failure {
            0 => driver
                .receive_bytes(&vec![0; needed + 1], 10, 10)
                .unwrap_err(),
            1 => driver.receive_bytes(&[0], -1, 10).unwrap_err(),
            _ => driver.receive_bytes(&[0], 11, 10).unwrap_err(),
        };
        assert!(driver.failed());
        assert_eq!(driver.revision(), before);
        assert_eq!(driver.participant_id(), id.0);
        assert!(driver.has_snapshot());
        assert_eq!(driver.take_start().unwrap_err(), first);
        assert_eq!(driver.receive_bytes(&[], 10, 10).unwrap_err(), first);
        no_final_receipts(&driver);
    }
    let (mut driver, registry, _) = admitted(10);
    let previous = driver.revision();
    let mut wrong = snapshot(&registry);
    let RoomMessage::Snapshot { members, .. } = &mut wrong else {
        unreachable!()
    };
    members[0].players[1] = PlayerId(91);
    let first = receive(&mut driver, &wrong, 11).unwrap_err();
    assert_eq!(driver.revision(), previous);
    assert!(driver.failed());
    assert_eq!(driver.request_leave().unwrap_err(), first);
    no_final_receipts(&driver);
}

#[test]
fn outbound_admission_requires_exact_completed_write_id_and_chronology() {
    for failure in 0..3 {
        let mut driver = driver();
        let frame = driver.next_write(100).unwrap().unwrap();
        assert_eq!(frame.id, 1);
        assert!(driver.next_write(100).unwrap().is_none());
        assert_eq!(driver.participant_id(), 0);
        assert_eq!(driver.revision(), 0);
        assert!(!driver.has_snapshot());
        no_final_receipts(&driver);
        let first = match failure {
            0 => driver.written(frame.id + 1, 100, 100).unwrap_err(),
            1 => driver.written(frame.id, 99, 100).unwrap_err(),
            _ => driver.written(frame.id, 101, 100).unwrap_err(),
        };
        assert_eq!(
            first,
            if failure == 0 {
                RoomPlayError::UnknownWrite
            } else {
                RoomPlayError::InvalidObservation
            }
        );
        assert!(driver.failed());
        assert_eq!(driver.written(frame.id, 100, 100).unwrap_err(), first);
        assert_eq!(driver.revision(), 0);
        no_final_receipts(&driver);
    }
}

#[test]
fn full_width_time_leave_receipt_and_idempotent_close_preserve_distinct_history() {
    let now = 9_007_199_254_740_993;
    let (mut driver, _, _) = admitted(now);
    assert_eq!(
        driver.session_ref().unwrap().room().unwrap().members[0].players,
        PLAYERS
    );
    driver.request_leave().unwrap();
    let leave = driver.next_write(i64::MAX).unwrap().unwrap();
    assert_eq!(decode_message(&leave.bytes).unwrap(), RoomMessage::Leave);
    assert!(!driver.leave_written());
    driver.written(leave.id, i64::MAX, i64::MAX).unwrap();
    assert!(driver.leave_written());
    no_final_receipts(&driver);
    let revision = driver.revision();
    driver.close();
    driver.close();
    assert!(driver.failed());
    assert_eq!(driver.revision(), revision);
    assert_eq!(driver.participant_id(), 0);
    assert!(!driver.has_snapshot() && !driver.frame_pending());
    assert_eq!(driver.session_ref().unwrap_err(), RoomPlayError::Stopped);
    assert_eq!(driver.needed_bytes().unwrap_err(), RoomPlayError::Stopped);
    assert_eq!(driver.pending_peer().unwrap_err(), RoomPlayError::Stopped);
    no_final_receipts(&driver);
}

fn measured_policy() -> StartPolicy {
    StartPolicy {
        lead_ns: 10_000,
        min_remaining_ns: 1_000,
        max_age_ns: 100_000,
        max_uncertainty_ns: 100,
        max_release_lateness_ns: 25,
    }
}

// Every server response below comes from the actual admission, clock or start
// owner. Only externally observed completed frames receive write receipts.
pub(crate) fn committed_pair() -> (Vec<RoomClientDriver>, GroupRoomRegistry, Vec<ParticipantId>) {
    committed_pair_with_setup(false)
}
fn committed_pair_with_setup(
    track_setup: bool,
) -> (Vec<RoomClientDriver>, GroupRoomRegistry, Vec<ParticipantId>) {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 2, 16, 1_000).unwrap());
    let mut clients = Vec::new();
    let mut ids = Vec::new();
    for roster in [PLAYERS, &[PlayerId(91)][..]] {
        let mut client = RoomClientDriver::new(IDENTITY, roster, measured_policy(), 0).unwrap();
        if track_setup {
            assert!(client.begin_setup(0, 1_000_000).is_ok());
            assert!(matches!(
                client.setup_step(0),
                Ok(RoomSetupStep::Wait(1_000_000))
            ));
        }
        let join = client.next_write(0).unwrap().unwrap();
        let RoomMessage::Join { identity, players } = decode_message(&join.bytes).unwrap() else {
            panic!("actual Join required")
        };
        client.written(join.id, 0, 0).unwrap();
        let id = registry.join("driver", &identity, &players, 0).unwrap().id;
        receive(&mut client, &RoomMessage::Admitted { participant: id }, 0).unwrap();
        if track_setup {
            assert!(matches!(
                client.setup_step(0),
                Ok(RoomSetupStep::Wait(1_000_000))
            ));
        }
        clients.push(client);
        ids.push(id);
        for client in &mut clients {
            receive(client, &snapshot(&registry), 0).unwrap();
            if track_setup {
                assert!(matches!(client.setup_step(0), Ok(RoomSetupStep::Idle)));
            }
        }
    }
    clients[0].request_seal().unwrap();
    let seal = clients[0].next_write(1).unwrap().unwrap();
    assert_eq!(decode_message(&seal.bytes).unwrap(), RoomMessage::Seal);
    clients[0].written(seal.id, 1, 1).unwrap();
    registry.seal(ids[0], 1).unwrap();
    for client in &mut clients {
        receive(client, &snapshot(&registry), 1).unwrap();
    }
    for index in 0..2 {
        clients[index].request_ready().unwrap();
        let ready = clients[index].next_write(2).unwrap().unwrap();
        assert_eq!(decode_message(&ready.bytes).unwrap(), RoomMessage::Ready);
        clients[index].written(ready.id, 2, 2).unwrap();
        registry.ready(ids[index], 2).unwrap();
        for client in &mut clients {
            receive(client, &snapshot(&registry), 2).unwrap();
        }
    }
    let room = registry.room("driver").unwrap();
    if track_setup {
        for client in &mut clients {
            assert!(matches!(
                client.setup_step(2),
                Ok(RoomSetupStep::Wait(1_000_000))
            ));
        }
    }
    // Prepared admission alone must not create a playback schedule or mutate
    // transport evidence, even under repeated finite startup observations.
    for client in &mut clients {
        let original = client.session_ref().unwrap().clone();
        for _ in 0..3 {
            assert!(client.take_start().unwrap().is_none());
        }
        assert_eq!(client.session_ref().unwrap(), &original);
    }
    let mut coordinator = RoomStartCoordinator::new(room, measured_policy()).unwrap();
    for index in 0..2 {
        let mut server = RoomClockExchange::new(room, ids[index]).unwrap();
        let client = &mut clients[index];
        for sequence in 1..=8 {
            let at = 10_000 + (sequence - 1) * 100;
            let ping = server.next(at).unwrap().unwrap();
            let local_ping = client.next_write(at).unwrap().unwrap();
            server.written(ping.id, at + 2).unwrap();
            client.written(local_ping.id, at + 3, at + 3).unwrap();
            server
                .receive(&decode_message(&local_ping.bytes).unwrap(), at + 7)
                .unwrap();
            receive(client, &decode_message(&ping.bytes).unwrap(), at + 11).unwrap();
            let pong = server.next(at + 13).unwrap().unwrap();
            let local_pong = client.next_write(at + 17).unwrap().unwrap();
            server.written(pong.id, at + 14).unwrap();
            client.written(local_pong.id, at + 18, at + 18).unwrap();
            server
                .receive(&decode_message(&local_pong.bytes).unwrap(), at + 23)
                .unwrap();
            receive(client, &decode_message(&pong.bytes).unwrap(), at + 29).unwrap();
            assert!(client.take_start().unwrap().is_none());
        }
        let estimate = server.estimate().unwrap();
        assert_eq!((estimate.lower_ns(), estimate.upper_ns()), (-6, 11));
        coordinator.prepare(ids[index], estimate, 10_800).unwrap();
    }
    for index in 0..2 {
        let ready = coordinator.next(ids[index], 10_800).unwrap().unwrap();
        assert_eq!(ready, StartMessage::ClockReady(0));
        coordinator.written(ids[index], ready, 10_800).unwrap();
        receive(&mut clients[index], &RoomMessage::Start(ready), 10_800).unwrap();
        let ready = clients[index].next_write(10_801).unwrap().unwrap();
        clients[index].written(ready.id, 10_802, 10_802).unwrap();
        let RoomMessage::Start(ready) = decode_message(&ready.bytes).unwrap() else {
            panic!("actual ClockReady required")
        };
        coordinator.receive(ids[index], ready, 10_803).unwrap();
    }
    for index in 0..2 {
        let proposal = coordinator
            .next(ids[index], 11_000 + index as i64)
            .unwrap()
            .unwrap();
        coordinator
            .written(ids[index], proposal, 11_000 + index as i64)
            .unwrap();
        receive(&mut clients[index], &RoomMessage::Start(proposal), 11_080).unwrap();
        let accept = clients[index].next_write(11_081).unwrap().unwrap();
        assert!(clients[index].take_start().unwrap().is_none());
        clients[index].written(accept.id, 11_082, 11_082).unwrap();
        let RoomMessage::Start(accept) = decode_message(&accept.bytes).unwrap() else {
            panic!("actual Accept required")
        };
        coordinator.receive(ids[index], accept, 11_090).unwrap();
    }
    for index in 0..2 {
        let commit = coordinator
            .next(ids[index], 11_100 + index as i64)
            .unwrap()
            .unwrap();
        assert!(matches!(commit, StartMessage::Commit(_)));
        coordinator
            .written(ids[index], commit, 11_101 + index as i64)
            .unwrap();
        receive(&mut clients[index], &RoomMessage::Start(commit), 11_200).unwrap();
    }
    assert!(coordinator.committed());
    (clients, registry, ids)
}

#[test]
fn genuine_start_and_relay_hold_peer_token_until_consumed_and_refuse_collision_atomically() {
    for consume in [false, true] {
        let (mut clients, registry, ids) = committed_pair();
        for client in &mut clients {
            let schedule = client
                .take_start()
                .unwrap()
                .expect("genuine Commit schedule");
            assert_eq!(schedule.target_ns, schedule.song_target_ns);
            assert_eq!(schedule.uncertainty_ns, 17);
            assert!(schedule.target_ns > 11_200);
            assert!(client.take_start().unwrap().is_none());
        }
        let mut relay = RoomProgressRelay::new(registry.room("driver").unwrap()).unwrap();
        relay.activate().unwrap();
        let revision = clients[1].revision();
        let mut first_prefix = None;
        for sequence in 1..=2 {
            let rows = PLAYERS
                .iter()
                .map(|&player| MemberProgress {
                    player,
                    progress: Progress {
                        song_ns: 9_007_199_254_740_993 + sequence,
                        hits: u64::MAX,
                        misses: 0,
                        combo: u64::MAX,
                        max_combo: u64::MAX,
                    },
                })
                .collect::<Vec<_>>();
            clients[0]
                .publish_progress_words(&encode_words(&rows).unwrap(), false)
                .unwrap();
            let at = 12_000 + sequence * 100;
            let upload = clients[0].next_write(at).unwrap().unwrap();
            let upload_message = decode_message(&upload.bytes).unwrap();
            relay.receive(ids[0], &upload_message).unwrap();
            clients[0].written(upload.id, at + 1, at + 1).unwrap();
            let frame = relay.poll_write(ids[1]).unwrap().unwrap();
            let message = decode_message(&frame.bytes).unwrap();
            let RoomMessage::PeerProgress {
                participant,
                prefix,
            } = &message
            else {
                panic!("actual relay peer frame required")
            };
            assert_eq!(*participant, ids[0]);
            assert_eq!(prefix.sequence, sequence as u64);
            assert_eq!(prefix.members, rows);
            let result = receive(&mut clients[1], &message, at + 10);
            relay.written(ids[1], frame.id).unwrap();
            assert_eq!(clients[1].revision(), revision);
            if sequence == 1 {
                result.unwrap();
                assert_eq!(clients[1].pending_peer().unwrap(), Some(ids[0]));
                let accepted = clients[1].peer_progress(ids[0]).unwrap();
                assert_eq!(accepted, prefix);
                let pointer = accepted as *const _;
                assert_eq!(clients[1].pending_peer().unwrap(), Some(ids[0]));
                assert_eq!(
                    clients[1].peer_progress(ids[0]).unwrap() as *const _,
                    pointer
                );
                first_prefix = Some(accepted.clone());
                if consume {
                    clients[1].consume_peer_progress();
                    assert!(clients[1].pending_peer().unwrap().is_none());
                    assert_eq!(clients[1].peer_progress(ids[0]), first_prefix.as_ref());
                }
            } else if consume {
                result.unwrap();
                assert!(!clients[1].failed());
                assert_eq!(clients[1].pending_peer().unwrap(), Some(ids[0]));
                assert_eq!(clients[1].peer_progress(ids[0]), Some(prefix));
                clients[1].consume_peer_progress();
                assert!(clients[1].pending_peer().unwrap().is_none());
            } else {
                assert_eq!(result.unwrap_err(), RoomPlayError::InvalidState);
                assert!(clients[1].failed());
                assert_eq!(clients[1].peer_progress(ids[0]), first_prefix.as_ref());
                assert_eq!(
                    clients[1].pending_peer().unwrap_err(),
                    RoomPlayError::InvalidState
                );
                assert_eq!(
                    clients[1].request_ready().unwrap_err(),
                    RoomPlayError::InvalidState
                );
            }
            no_final_receipts(&clients[1]);
        }
    }
}

#[test]
fn drain_setup_bounds_are_atomic_and_repeated_begin_does_not_renew_fixed_deadline() {
    for (now, timeout) in [
        (-1, 1_000_000),
        (0, 999_999),
        (0, 120_000_000_001),
        (0, u64::MAX),
        (i64::MAX - 500_000, 1_000_000),
    ] {
        let mut client = driver();
        assert!(client.begin_drain(now, timeout).is_err());
        assert!(!client.drain_requested());
        assert!(!client.failed());
        // Invalid setup must not consume or renew a wait owner.
        assert!(client.begin_drain(0, 1_000_000).is_ok());
    }
    let (mut clients, _, _) = committed_pair();
    let client = &mut clients[0];
    assert!(client.begin_drain(12_000, 1_000_000).is_ok());
    assert!(matches!(
        client.begin_drain(900_000, 120_000_000_000),
        Err(RoomDrainError::InvalidState)
    ));
    assert!(matches!(
        client.drain_step(12_000),
        Ok(RoomFinalStep::Wait(1_000_000))
    ));
    assert!(!client.drain_requested());
    assert!(
        client.next_write(12_000).unwrap().is_none(),
        "drain wait cannot publish a synthetic final"
    );
    assert!(matches!(
        client.drain_step(1_012_000),
        Err(RoomDrainError::TimedOut)
    ));
    assert!(matches!(
        client.drain_step(1_012_001),
        Err(RoomDrainError::InvalidTerminal)
    ));
    no_final_receipts(client);
}

#[test]
fn drain_clock_refusal_leave_and_close_never_create_terminal_success() {
    for now in [-1, 11_999] {
        let (mut clients, _, _) = committed_pair();
        let client = &mut clients[0];
        assert!(client.begin_drain(12_000, 1_000_000).is_ok());
        assert!(matches!(
            client.drain_step(12_000),
            Ok(RoomFinalStep::Wait(_))
        ));
        let result = client.drain_step(now);
        if now < 0 {
            assert!(matches!(result, Err(RoomDrainError::InvalidClock)));
        } else {
            assert!(matches!(result, Err(RoomDrainError::ClockRegressed)));
        }
        assert!(matches!(
            client.drain_step(12_001),
            Err(RoomDrainError::InvalidTerminal)
        ));
        assert!(!client.drain_requested());
        no_final_receipts(client);
    }
    let (mut clients, _, _) = committed_pair();
    let client = &mut clients[0];
    assert!(client.begin_drain(12_000, 1_000_000).is_ok());
    client.request_leave().unwrap();
    assert!(matches!(
        client.drain_step(12_000),
        Err(RoomDrainError::InvalidTerminal)
    ));
    assert!(!client.drain_requested());
    client.close();
    assert!(client.begin_drain(12_001, 1_000_000).is_err());
    assert!(client.drain_step(12_001).is_err());
    no_final_receipts(client);
    // Signed room and unsigned scheduling values retain full precision.
    let mut client = driver();
    let now = 9_007_199_254_740_993;
    assert!(client.begin_drain(now, 1_000_000).is_ok());
    assert!(matches!(
        client.drain_step(now),
        Ok(RoomFinalStep::Wait(1_000_000))
    ));
    assert!(matches!(
        client.drain_step(now + 1_000_000),
        Err(RoomDrainError::TimedOut)
    ));
}

#[test]
fn genuine_final_upload_ack_and_drain_complete_are_the_only_success_evidence() {
    let (mut clients, registry, ids) = committed_pair();
    let room = registry.room("driver").unwrap();
    let mut relay = RoomProgressRelay::new(room).unwrap();
    relay.activate().unwrap();
    for index in 0..2 {
        let rows = room.members[index]
            .players
            .iter()
            .map(|&player| MemberProgress {
                player,
                progress: Progress {
                    song_ns: 604_800_000_000_000,
                    hits: u64::MAX,
                    misses: 0,
                    combo: u64::MAX,
                    max_combo: u64::MAX,
                },
            })
            .collect::<Vec<_>>();
        clients[index]
            .publish_progress_words(&encode_words(&rows).unwrap(), true)
            .unwrap();
        assert!(!clients[index].local_final_written());
        let frame = clients[index].next_write(12_000).unwrap().unwrap();
        let actual = decode_message(&frame.bytes).unwrap();
        assert!(matches!(actual, RoomMessage::Progress(_)));
        relay.receive(ids[index], &actual).unwrap();
        clients[index].written(frame.id, 12_001, 12_001).unwrap();
        assert!(clients[index].local_final_written());
        assert!(!clients[index].local_final_acknowledged());
    }
    for turn in 0..12 {
        let at = 13_000 + turn * 100;
        for index in 0..2 {
            if let Some(frame) = relay.poll_write(ids[index]).unwrap() {
                let message = decode_message(&frame.bytes).unwrap();
                receive(&mut clients[index], &message, at + 10).unwrap();
                clients[index].consume_peer_progress();
                relay.written(ids[index], frame.id).unwrap();
            }
        }
        for index in 0..2 {
            if let Some(frame) = clients[index].next_write(at + 20).unwrap() {
                let message = decode_message(&frame.bytes).unwrap();
                assert!(matches!(message, RoomMessage::FinalAck { .. }));
                relay.receive(ids[index], &message).unwrap();
                clients[index].written(frame.id, at + 21, at + 21).unwrap();
            }
        }
        if relay.complete() && clients.iter().all(RoomClientDriver::progress_complete) {
            break;
        }
    }
    assert!(relay.complete());
    assert!(clients.iter().all(RoomClientDriver::progress_complete));
    for index in 0..2 {
        assert!(clients[index].begin_drain(19_000, 1_000_000).is_ok());
        assert!(matches!(
            clients[index].drain_step(19_000),
            Ok(RoomFinalStep::Wait(_))
        ));
        assert!(clients[index].drain_requested());
        assert!(!clients[index].drain_complete());
        let frame = clients[index].next_write(19_001).unwrap().unwrap();
        let message = decode_message(&frame.bytes).unwrap();
        assert!(
            matches!(message, RoomMessage::DrainReady { .. }),
            "already-admitted final must not be published twice"
        );
        relay.receive(ids[index], &message).unwrap();
        clients[index].written(frame.id, 19_002, 19_002).unwrap();
    }
    for index in 0..2 {
        let frame = relay
            .poll_write(ids[index])
            .unwrap()
            .expect("genuine DrainComplete");
        let message = decode_message(&frame.bytes).unwrap();
        assert!(matches!(message, RoomMessage::DrainComplete { .. }));
        receive(&mut clients[index], &message, 19_010).unwrap();
        relay.written(ids[index], frame.id).unwrap();
        assert!(clients[index].local_final_written() && clients[index].local_final_acknowledged());
        assert!(clients[index].progress_complete() && clients[index].drain_complete());
        assert!(matches!(
            clients[index].drain_step(19_010),
            Ok(RoomFinalStep::Completed)
        ));
        assert!(matches!(
            clients[index].drain_step(i64::MAX),
            Ok(RoomFinalStep::Completed)
        ));
    }
}

#[test]
fn pending_start_observations_preserve_fresh_and_admitted_protocol_write_evidence() {
    let fresh = driver();
    let (admitted, _, _) = admitted(0);
    for mut client in [fresh, admitted] {
        let original = client.session_ref().unwrap().clone();
        let revision = client.revision();
        for _ in 0..4 {
            assert!(client.take_start().unwrap().is_none());
            assert_eq!(client.session_ref().unwrap(), &original);
            assert_eq!(client.revision(), revision);
            no_final_receipts(&client);
        }
    }
}

#[test]
fn startup_steps_return_exact_original_common_commit_schedule_only_once() {
    let (mut clients, _, _) = committed_pair();
    for client in &mut clients {
        let mut common = client.session_ref().unwrap().clone();
        let expected = common
            .take_schedule()
            .expect("actual common Commit schedule");
        let revision = client.revision();
        let actual = client
            .take_start()
            .unwrap()
            .expect("genuine driver schedule");
        assert_eq!(actual, expected);
        assert_eq!(actual.uncertainty_ns, 17);
        assert_eq!(actual.target_ns, actual.song_target_ns);
        assert_eq!(client.revision(), revision);
        for _ in 0..4 {
            assert!(client.take_start().unwrap().is_none());
        }
        no_final_receipts(client);
        assert!(!client.failed());
    }
}

#[test]
fn leave_cancels_unconsumed_commit_and_health_guards_precede_ready_or_pending_start_state() {
    let (mut clients, _, _) = committed_pair();
    let client = &mut clients[0];
    assert!(
        client
            .session_ref()
            .unwrap()
            .clone()
            .take_schedule()
            .is_some()
    );
    client.request_leave().unwrap();
    for _ in 0..3 {
        assert!(client.take_start().unwrap().is_none());
    }
    let frame = client.next_write(12_000).unwrap().unwrap();
    assert_eq!(decode_message(&frame.bytes).unwrap(), RoomMessage::Leave);
    assert!(!client.leave_written());
    client.written(frame.id, 12_001, 12_001).unwrap();
    assert!(client.leave_written());
    assert!(client.take_start().unwrap().is_none());
    no_final_receipts(client);

    for consume_first in [false, true] {
        for close in [false, true] {
            let (mut clients, _, _) = committed_pair();
            let client = &mut clients[0];
            assert!(
                client
                    .session_ref()
                    .unwrap()
                    .clone()
                    .take_schedule()
                    .is_some()
            );
            if consume_first {
                assert!(client.take_start().unwrap().is_some());
            }
            let first = if close {
                client.close();
                RoomPlayError::Stopped
            } else {
                let oversized = vec![0; client.needed_bytes().unwrap() + 1];
                client
                    .receive_bytes(&oversized, 12_000, 12_000)
                    .unwrap_err()
            };
            for _ in 0..3 {
                assert_eq!(client.take_start().unwrap_err(), first);
                assert_eq!(client.session_ref().unwrap_err(), first);
            }
            assert!(client.failed());
            no_final_receipts(client);
        }
    }
}

#[test]
fn setup_real_protocol_phases_preserve_unconsumed_schedule_and_fixed_prepared_deadline() {
    for late in [false, true] {
        let (mut clients, _, _) = committed_pair_with_setup(true);
        for client in &mut clients {
            let expected = client
                .session_ref()
                .unwrap()
                .clone()
                .take_schedule()
                .expect("actual Commit");
            assert!(matches!(
                client.begin_setup(11_200, 120_000_000_000),
                Err(RoomClientSetupError::InvalidState)
            ));
            if late {
                assert!(matches!(
                    client.setup_step(1_000_002),
                    Err(RoomClientSetupError::Policy(RoomSetupError::Deadline(
                        RoomSetupPhase::Prepared,
                        RoomDeadlineError::Expired
                    )))
                ));
                assert!(matches!(
                    client.setup_step(1_000_003),
                    Err(RoomClientSetupError::Policy(RoomSetupError::Finished))
                ));
            } else {
                assert!(matches!(
                    client.setup_step(11_200),
                    Ok(RoomSetupStep::Complete)
                ));
                assert!(matches!(
                    client.setup_step(i64::MAX),
                    Ok(RoomSetupStep::Complete)
                ));
                // Setup observes commitment without consuming the common one-shot.
                assert_eq!(client.take_start().unwrap(), Some(expected));
                assert!(client.take_start().unwrap().is_none());
            }
            no_final_receipts(client);
        }
    }
}

#[test]
fn setup_late_real_admission_and_lifetime_refusals_never_promote_completion() {
    let mut client = driver();
    assert!(client.begin_setup(-1, 1_000_000).is_err());
    assert!(client.begin_setup(0, 1_000_000).is_ok());
    let join = client.next_write(0).unwrap().unwrap();
    let RoomMessage::Join { identity, players } = decode_message(&join.bytes).unwrap() else {
        panic!("Join")
    };
    client.written(join.id, 0, 0).unwrap();
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 2, 16, 1_000).unwrap());
    let id = registry.join("driver", &identity, &players, 0).unwrap().id;
    receive(
        &mut client,
        &RoomMessage::Admitted { participant: id },
        1_000_000,
    )
    .unwrap();
    receive(&mut client, &snapshot(&registry), 1_000_000).unwrap();
    assert_eq!(
        client.participant_id(),
        id.0,
        "late admission remains history"
    );
    assert!(matches!(
        client.setup_step(1_000_000),
        Err(RoomClientSetupError::Policy(RoomSetupError::Deadline(
            RoomSetupPhase::Admission,
            RoomDeadlineError::Expired
        )))
    ));
    assert!(matches!(
        client.setup_step(1_000_001),
        Err(RoomClientSetupError::Policy(RoomSetupError::Finished))
    ));
    assert!(client.take_start().unwrap().is_none());
    for close in [false, true] {
        let (mut clients, _, _) = committed_pair_with_setup(true);
        let client = &mut clients[0];
        if close {
            client.close();
        } else {
            client.request_leave().unwrap();
        }
        assert!(client.setup_step(11_200).is_err());
        assert!(client.begin_setup(11_200, 1_000_000).is_err());
        no_final_receipts(client);
    }
}

#[test]
fn frame_wait_uses_real_header_body_prefix_and_refuses_late_admission_before_mutation() {
    for late in [false, true] {
        let mut client = driver();
        assert!(client.configure_frame_wait(1_000_000).is_ok());
        let join = client.next_write(0).unwrap().unwrap();
        let RoomMessage::Join { identity, players } = decode_message(&join.bytes).unwrap() else {
            panic!("Join")
        };
        client.written(join.id, 0, 0).unwrap();
        let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 2, 16, 1_000).unwrap());
        let id = registry.join("driver", &identity, &players, 0).unwrap().id;
        let bytes = encode_message(&RoomMessage::Admitted { participant: id }).unwrap();
        let header = client.needed_bytes().unwrap();
        assert_eq!(
            client.receive_bytes(&bytes[..header], 10, 10).unwrap(),
            header
        );
        assert!(client.frame_pending());
        assert!(matches!(
            client.frame_wait_step(10),
            Ok(RoomFrameWaitStep::Wait(1_000_000))
        ));
        assert_eq!(client.revision(), 0);
        assert_eq!(client.participant_id(), 0);
        let now = if late { 1_000_010 } else { 1_000_009 };
        let result = client.receive_bytes(&bytes[header..], now, now);
        if late {
            let first = result.unwrap_err();
            assert_eq!(
                first,
                RoomPlayError::FrameWait(RoomFrameWaitError::Deadline(RoomDeadlineError::Expired))
            );
            assert!(client.failed());
            assert_eq!(client.revision(), 0);
            assert_eq!(client.participant_id(), 0);
            assert_eq!(client.receive_bytes(&[], now, now).unwrap_err(), first);
            assert_eq!(client.frame_wait_step(now).unwrap_err(), first);
        } else {
            assert_eq!(result.unwrap(), bytes.len() - header);
            assert_eq!(client.revision(), 1);
            assert_eq!(client.participant_id(), id.0);
            assert!(matches!(
                client.frame_wait_step(now),
                Ok(RoomFrameWaitStep::Idle)
            ));
            let next = encode_message(&snapshot(&registry)).unwrap();
            let header = client.needed_bytes().unwrap();
            let at = 604_800_000_000_000;
            client.receive_bytes(&next[..header], at, at).unwrap();
            assert!(matches!(
                client.frame_wait_step(at),
                Ok(RoomFrameWaitStep::Wait(1_000_000))
            ));
            client
                .receive_bytes(&next[header..], at + 999_999, at + 999_999)
                .unwrap();
            assert_eq!(client.revision(), 2);
            assert!(client.has_snapshot());
        }
        no_final_receipts(&client);
    }
}

#[test]
fn frame_wait_configuration_is_once_empty_live_and_cannot_replace_a_partial_deadline() {
    let mut client = driver();
    assert!(client.configure_frame_wait(0).is_err());
    assert!(!client.failed());
    assert!(client.configure_frame_wait(1_000_000).is_ok());
    assert_eq!(
        client.configure_frame_wait(120_000_000_000).unwrap_err(),
        RoomPlayError::InvalidState
    );
    assert!(matches!(
        client.frame_wait_step(0),
        Ok(RoomFrameWaitStep::Idle)
    ));
    client.receive_bytes(&[b'B'], 1, 1).unwrap();
    assert!(matches!(
        client.frame_wait_step(2),
        Ok(RoomFrameWaitStep::Wait(999_999))
    ));
    assert_eq!(
        client.configure_frame_wait(120_000_000_000).unwrap_err(),
        RoomPlayError::InvalidState
    );
    let first = client.frame_wait_step(1_000_001).unwrap_err();
    assert_eq!(
        first,
        RoomPlayError::FrameWait(RoomFrameWaitError::Deadline(RoomDeadlineError::Expired))
    );
    assert_eq!(client.configure_frame_wait(1_000_000).unwrap_err(), first);
    client.close();
    assert_eq!(client.configure_frame_wait(1_000_000).unwrap_err(), first);
    let mut unconfigured = driver();
    unconfigured.receive_bytes(&[b'B'], 0, 0).unwrap();
    assert_eq!(
        unconfigured.configure_frame_wait(1_000_000).unwrap_err(),
        RoomPlayError::InvalidState
    );
    assert!(!unconfigured.failed());
}

#[test]
fn admitted_leave_cancels_partial_frame_without_decoder_mutation_or_synthetic_write_receipt() {
    for wrong_receipt in [false, true] {
        let (mut client, registry, _) = admitted(0);
        client.configure_frame_wait(1_000_000).unwrap();
        let bytes = encode_message(&snapshot(&registry)).unwrap();
        let header = client.needed_bytes().unwrap();
        client.receive_bytes(&bytes[..header], 10, 10).unwrap();
        let revision = client.revision();
        let participant = client.participant_id();
        let remaining = client.needed_bytes().unwrap();
        assert!(client.frame_pending());
        client.request_leave().unwrap();
        assert!(client.session_ref().unwrap().leave_requested());
        assert!(!client.leave_written());
        for now in [-1, 1_000_010, i64::MAX] {
            assert!(matches!(
                client.frame_wait_step(now),
                Ok(RoomFrameWaitStep::Idle)
            ));
            assert_eq!(
                client
                    .receive_bytes(&bytes[header..], now, now)
                    .unwrap_err(),
                RoomPlayError::InvalidState
            );
            assert!(!client.failed());
            assert_eq!(client.revision(), revision);
            assert_eq!(client.participant_id(), participant);
            assert!(client.frame_pending());
            assert_eq!(client.needed_bytes().unwrap(), remaining);
            assert!(client.pending_peer().unwrap().is_none());
        }
        assert_eq!(
            client.configure_frame_wait(1_000_000).unwrap_err(),
            RoomPlayError::InvalidState
        );
        let leave = client
            .next_write(1_000_010)
            .unwrap()
            .expect("actual outgoing Leave");
        assert_eq!(decode_message(&leave.bytes).unwrap(), RoomMessage::Leave);
        assert!(client.next_write(1_000_010).unwrap().is_none());
        assert!(!client.leave_written());
        if wrong_receipt {
            let first = client
                .written(leave.id + 1, 1_000_010, 1_000_010)
                .unwrap_err();
            assert_eq!(first, RoomPlayError::UnknownWrite);
            assert!(client.failed());
            assert_eq!(client.frame_wait_step(-1).unwrap_err(), first);
            assert_eq!(
                client.written(leave.id, 1_000_010, 1_000_010).unwrap_err(),
                first
            );
            assert!(!client.leave_written());
        } else {
            client.written(leave.id, 1_000_010, 1_000_010).unwrap();
            assert!(client.leave_written());
            assert!(matches!(
                client.frame_wait_step(-1),
                Ok(RoomFrameWaitStep::Idle)
            ));
        }
        no_final_receipts(&client);
    }
}

#[test]
fn refused_leave_retains_original_partial_deadline_and_failed_owner_cannot_revive() {
    let mut client = driver();
    client.configure_frame_wait(1_000_000).unwrap();
    let join = client.next_write(0).unwrap().unwrap();
    let RoomMessage::Join { identity, players } = decode_message(&join.bytes).unwrap() else {
        panic!("Join")
    };
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 2, 16, 1_000).unwrap());
    let id = registry.join("driver", &identity, &players, 0).unwrap().id;
    let bytes = encode_message(&RoomMessage::Admitted { participant: id }).unwrap();
    let header = client.needed_bytes().unwrap();
    client.receive_bytes(&bytes[..header], 10, 10).unwrap();
    assert!(
        client.request_leave().is_err(),
        "in-flight Join has no completed write authority"
    );
    assert!(!client.failed());
    assert!(!client.session_ref().unwrap().leave_requested());
    assert!(matches!(
        client.frame_wait_step(11),
        Ok(RoomFrameWaitStep::Wait(999_999))
    ));
    let first = client.frame_wait_step(1_000_010).unwrap_err();
    assert_eq!(
        first,
        RoomPlayError::FrameWait(RoomFrameWaitError::Deadline(RoomDeadlineError::Expired))
    );
    assert_eq!(client.request_leave().unwrap_err(), first);
    assert_eq!(client.frame_wait_step(-1).unwrap_err(), first);
    assert_eq!(
        client
            .receive_bytes(&bytes[header..], 1_000_010, 1_000_010)
            .unwrap_err(),
        first
    );
    assert_eq!(client.participant_id(), 0);
    assert_eq!(client.revision(), 0);
    assert!(!client.leave_written());
}
