//! Deferred native-owner fixtures. Scripted byte streams exercise the real
//! common admission/clock/start/progress owners, without sockets or audio.
use super::*;
use crate::{
    local_players::PlayerId,
    multiplayer_group::MemberProgress,
    multiplayer_group_rooms::{GroupRoomPhase, GroupRoomPolicy, GroupRoomRegistry},
    multiplayer_protocol::Progress,
    multiplayer_room_clock::RoomClockExchange,
    multiplayer_room_progress::RoomProgressRelay,
    multiplayer_room_start::RoomStartCoordinator,
    multiplayer_room_wire::{RoomMessage, decode_message, encode_message},
    multiplayer_start::{StartMessage, StartPolicy},
};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    sync::Mutex,
};

const IDENTITY: &[u8] = &[0, 255, 17];

fn policy() -> StartPolicy {
    StartPolicy {
        lead_ns: 10_000,
        min_remaining_ns: 1_000,
        max_age_ns: 100_000,
        max_uncertainty_ns: 100,
        max_release_lateness_ns: 25,
    }
}

#[derive(Clone, Copy)]
enum Action {
    Limit(usize),
    Error(io::ErrorKind),
}

#[derive(Default)]
struct Script {
    input: VecDeque<u8>,
    output: Vec<u8>,
    reads: VecDeque<Action>,
    writes: VecDeque<Action>,
    read_calls: usize,
    write_calls: usize,
    read_extents: Vec<usize>,
    finish_calls: usize,
    drops: usize,
    cleanup_error: bool,
    idle_calls: usize,
}
struct Stream(Arc<Mutex<Script>>);
impl Drop for Stream {
    fn drop(&mut self) {
        self.0.lock().unwrap().drops += 1;
    }
}
impl Read for Stream {
    fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
        let mut script = self.0.lock().unwrap();
        script.read_calls += 1;
        script.read_extents.push(target.len());
        match script
            .reads
            .pop_front()
            .unwrap_or(Action::Limit(usize::MAX))
        {
            Action::Error(kind) => Err(io::Error::new(kind, "scripted read")),
            Action::Limit(0) => Ok(0),
            Action::Limit(limit) => {
                if script.input.is_empty() {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                let count = limit.min(target.len()).min(script.input.len());
                for value in &mut target[..count] {
                    *value = script.input.pop_front().unwrap();
                }
                Ok(count)
            }
        }
    }
}
impl Write for Stream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut script = self.0.lock().unwrap();
        script.write_calls += 1;
        match script
            .writes
            .pop_front()
            .unwrap_or(Action::Limit(usize::MAX))
        {
            Action::Error(kind) => Err(io::Error::new(kind, "scripted write")),
            Action::Limit(limit) => {
                let count = limit.min(bytes.len());
                script.output.extend_from_slice(&bytes[..count]);
                Ok(count)
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        panic!("the common driver must not flush or wait")
    }
}
impl NativeRoomStream for Stream {
    fn idle(&mut self, _: Duration) -> io::Result<()> {
        self.0.lock().unwrap().idle_calls += 1;
        Ok(())
    }
    fn finish(&mut self, _: Duration) -> io::Result<()> {
        let mut script = self.0.lock().unwrap();
        script.finish_calls += 1;
        if script.cleanup_error {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "separate stream cleanup failure",
            ))
        } else {
            Ok(())
        }
    }
}

fn options(preroll_ns: i64) -> NativeRoomOptions {
    NativeRoomOptions {
        setup_timeout: Duration::from_secs(1),
        drain_timeout: Duration::from_millis(1),
        finish_timeout: Duration::from_millis(10),
        frame_timeout: Duration::from_secs(10),
        queue_capacity: 2,
        start_policy: policy(),
        preroll_ns,
    }
}

struct Endpoint {
    actor: Actor<Stream>,
    script: Arc<Mutex<Script>>,
}
fn endpoint(players: &[PlayerId], preroll: i64) -> Endpoint {
    let script = Arc::new(Mutex::new(Script::default()));
    let io = RoomPlayIo::new(
        RoomPlayClient::new(IDENTITY, players, policy(), preroll).unwrap(),
        Stream(script.clone()),
    );
    Endpoint {
        actor: Actor::new(io, options(preroll)).unwrap(),
        script,
    }
}
fn step(endpoint: &mut Endpoint, now: i64) -> bool {
    let before = {
        let script = endpoint.script.lock().unwrap();
        (script.read_calls, script.write_calls)
    };
    let moved = endpoint.actor.drive(|| Ok(now)).unwrap();
    let script = endpoint.script.lock().unwrap();
    assert!(script.read_calls - before.0 <= 1);
    assert!(script.write_calls - before.1 <= 1);
    assert!(
        script
            .read_extents
            .iter()
            .all(|&extent| (1..=4096).contains(&extent))
    );
    moved
}
fn deliver(endpoint: &mut Endpoint, message: &RoomMessage, now: i64) {
    queued(&endpoint.script, message);
    for _ in 0..32 {
        if endpoint.script.lock().unwrap().input.is_empty() {
            return;
        }
        assert!(step(endpoint, now));
    }
    panic!("bounded actual room frame did not drain");
}

fn emitted(script: &Arc<Mutex<Script>>) -> RoomMessage {
    let bytes = std::mem::take(&mut script.lock().unwrap().output);
    decode_message(&bytes).unwrap()
}
fn queued(script: &Arc<Mutex<Script>>, message: &RoomMessage) {
    script
        .lock()
        .unwrap()
        .input
        .extend(encode_message(message).unwrap());
}
fn snapshot(registry: &GroupRoomRegistry) -> RoomMessage {
    let room = registry.room("room").unwrap();
    RoomMessage::Snapshot {
        members: room.members.to_vec(),
        phase: room.phase,
        deadline_ns: room.deadline_ns,
    }
}
fn progress(players: &[PlayerId], hits: u64) -> Vec<MemberProgress> {
    players
        .iter()
        .map(|&player| MemberProgress {
            player,
            progress: Progress {
                song_ns: 604_800_000_000_000,
                hits,
                misses: 0,
                combo: hits,
                max_combo: hits,
            },
        })
        .collect()
}

struct Cohort {
    clients: Vec<Endpoint>,
    registry: GroupRoomRegistry,
    ids: Vec<ParticipantId>,
}
fn collecting(count: usize) -> Cohort {
    let mut cohort = Cohort {
        clients: Vec::new(),
        ids: Vec::new(),
        registry: GroupRoomRegistry::new(GroupRoomPolicy::new(1, count, 8, 1_000).unwrap()),
    };
    for index in 0..count {
        let players = (0..=index)
            .map(|slot| PlayerId(u32::MAX - slot as u32))
            .collect::<Vec<_>>();
        let mut client = endpoint(&players, index as i64 * 100);
        assert!(step(&mut client, 0));
        let RoomMessage::Join {
            identity,
            players: actual,
        } = emitted(&client.script)
        else {
            panic!("actual Join required")
        };
        assert_eq!(actual, players);
        let id = cohort
            .registry
            .join("room", &identity, &actual, 0)
            .unwrap()
            .id;
        deliver(&mut client, &RoomMessage::Admitted { participant: id }, 0);
        cohort.ids.push(id);
        cohort.clients.push(client);
        let value = snapshot(&cohort.registry);
        for client in &mut cohort.clients {
            deliver(client, &value, 0);
        }
    }
    cohort
}
fn prepared(count: usize) -> Cohort {
    let mut cohort = collecting(count);
    cohort.clients[0]
        .actor
        .command(NativeRoomCommand::Seal, 1)
        .unwrap();
    step(&mut cohort.clients[0], 1);
    assert_eq!(emitted(&cohort.clients[0].script), RoomMessage::Seal);
    cohort.registry.seal(cohort.ids[0], 1).unwrap();
    let value = snapshot(&cohort.registry);
    for client in &mut cohort.clients {
        deliver(client, &value, 1);
    }
    for index in 0..count {
        cohort.clients[index]
            .actor
            .command(NativeRoomCommand::Ready, 2)
            .unwrap();
        step(&mut cohort.clients[index], 2);
        assert_eq!(emitted(&cohort.clients[index].script), RoomMessage::Ready);
        cohort.registry.ready(cohort.ids[index], 2).unwrap();
        let value = snapshot(&cohort.registry);
        for client in &mut cohort.clients {
            deliver(client, &value, 2);
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

fn committed(count: usize) -> (Cohort, i64) {
    let mut cohort = prepared(count);
    let room = cohort.registry.room("room").unwrap();
    let mut clocks = cohort
        .ids
        .iter()
        .map(|id| RoomClockExchange::new(room, *id).unwrap())
        .collect::<Vec<_>>();
    let mut start = RoomStartCoordinator::new(room, policy()).unwrap();
    for index in 0..count {
        let client = &mut cohort.clients[index];
        let shift = offset(index);
        for sequence in 1..=8 {
            let at = 10_000 + (sequence as i64 - 1) * 100;
            let ping = clocks[index].next(at).unwrap().unwrap();
            step(client, at + shift);
            let actual_ping = emitted(&client.script);
            assert_eq!(
                actual_ping,
                RoomMessage::ClockPing {
                    sequence,
                    sent_ns: at + shift
                }
            );
            clocks[index].written(ping.id, at + 2).unwrap();
            clocks[index].receive(&actual_ping, at + 7).unwrap();
            deliver(
                client,
                &decode_message(&ping.bytes).unwrap(),
                at + shift + 11,
            );
            let pong = clocks[index].next(at + 13).unwrap().unwrap();
            clocks[index].written(pong.id, at + 14).unwrap();
            step(client, at + shift + 17);
            let actual_pong = emitted(&client.script);
            assert_eq!(
                actual_pong,
                RoomMessage::ClockPong {
                    sequence,
                    sent_ns: at,
                    received_ns: at + shift + 11,
                    replied_ns: at + shift + 17
                }
            );
            clocks[index].receive(&actual_pong, at + 23).unwrap();
            deliver(
                client,
                &decode_message(&pong.bytes).unwrap(),
                at + shift + 29,
            );
            assert!(client.actor.snapshot().schedule.is_none());
        }
        let estimate = clocks[index].estimate().unwrap();
        assert_eq!(
            (estimate.lower_ns(), estimate.upper_ns()),
            (i128::from(shift - 6), i128::from(shift + 11))
        );
        start.prepare(cohort.ids[index], estimate, 10_800).unwrap();
    }
    for index in 0..count {
        let ready = start.next(cohort.ids[index], 10_800).unwrap().unwrap();
        assert_eq!(ready, StartMessage::ClockReady(0));
        start.written(cohort.ids[index], ready, 10_800).unwrap();
        deliver(
            &mut cohort.clients[index],
            &RoomMessage::Start(ready),
            10_800 + offset(index),
        );
    }
    for index in 0..count {
        let RoomMessage::Start(ready) = emitted(&cohort.clients[index].script) else {
            panic!("actual ClockReady required")
        };
        assert_eq!(ready, StartMessage::ClockReady(index as i64 * 100));
        start.receive(cohort.ids[index], ready, 10_810).unwrap();
    }
    let target = 21_000 + (count as i64 - 1) * 100 + 17;
    for index in 0..count {
        let proposal = start
            .next(cohort.ids[index], 11_000 + index as i64)
            .unwrap()
            .unwrap();
        assert_eq!(proposal, StartMessage::Propose(target));
        start
            .written(cohort.ids[index], proposal, 11_000 + index as i64)
            .unwrap();
        deliver(
            &mut cohort.clients[index],
            &RoomMessage::Start(proposal),
            11_080 + offset(index),
        );
    }
    for index in 0..count {
        step(&mut cohort.clients[index], 11_081 + offset(index));
        let RoomMessage::Start(accept) = emitted(&cohort.clients[index].script) else {
            panic!("actual Accept required")
        };
        assert_eq!(accept, StartMessage::Accept(target));
        assert!(cohort.clients[index].actor.snapshot().schedule.is_none());
        start.receive(cohort.ids[index], accept, 11_090).unwrap();
        if index + 1 < count {
            assert!(start.next(cohort.ids[0], 11_090).unwrap().is_none());
        }
    }
    for index in 0..count {
        let commit = start
            .next(cohort.ids[index], 11_100 + index as i64)
            .unwrap()
            .unwrap();
        assert_eq!(commit, StartMessage::Commit(target));
        assert!(!start.committed());
        start
            .written(cohort.ids[index], commit, 11_101 + index as i64)
            .unwrap();
        deliver(
            &mut cohort.clients[index],
            &RoomMessage::Start(commit),
            11_200 + offset(index),
        );
    }
    assert!(start.committed());
    (cohort, target)
}

fn locally_complete(count: usize) -> (Cohort, RoomProgressRelay, i64) {
    let (mut cohort, _) = committed(count);
    let room = cohort.registry.room("room").unwrap();
    let mut relay = RoomProgressRelay::new(room).unwrap();
    relay.activate().unwrap();
    for index in 0..count {
        let client = &mut cohort.clients[index];
        client
            .actor
            .command(
                NativeRoomCommand::Publish {
                    members: progress(&room.members[index].players, u64::MAX),
                    final_prefix: true,
                },
                12_000 + offset(index),
            )
            .unwrap();
        assert!(!client.actor.snapshot().receipts.local_final_written);
        step(client, 12_000 + offset(index));
        let upload = emitted(&client.script);
        assert!(matches!(upload, RoomMessage::Progress(_)));
        relay
            .receive_at(cohort.ids[index], &upload, 12_010)
            .unwrap();
        assert!(client.actor.snapshot().receipts.local_final_written);
        assert!(!client.actor.snapshot().receipts.local_final_acknowledged);
    }
    for turn in 0..count + 2 {
        let at = 13_000 + turn as i64 * 100;
        for index in 0..count {
            if let Some(frame) = relay.poll_write_at(cohort.ids[index], at).unwrap() {
                let message = decode_message(&frame.bytes).unwrap();
                assert!(matches!(
                    message,
                    RoomMessage::PeerProgress { .. } | RoomMessage::FinalAck { .. }
                ));
                deliver(
                    &mut cohort.clients[index],
                    &message,
                    at + offset(index) + 10,
                );
                relay.written(cohort.ids[index], frame.id).unwrap();
            }
        }
        for index in 0..count {
            let client = &mut cohort.clients[index];
            step(client, at + offset(index) + 20);
            if !client.script.lock().unwrap().output.is_empty() {
                let ack = emitted(&client.script);
                assert!(matches!(ack, RoomMessage::FinalAck { .. }));
                relay.receive_at(cohort.ids[index], &ack, at + 30).unwrap();
            }
        }
        if relay.complete()
            && cohort
                .clients
                .iter()
                .all(|client| client.actor.snapshot().receipts.progress_complete)
        {
            assert!(
                cohort.clients.iter().all(|client| !client
                    .actor
                    .snapshot()
                    .receipts
                    .drain_complete)
            );
            return (cohort, relay, at + 100);
        }
    }
    panic!("bounded real final/ACK exchange did not complete");
}

#[test]
fn invalid_native_setup_is_refused_before_connector_or_stream_ownership() {
    let players = [PlayerId(7)];
    let mut bad_options = Vec::new();
    let mut value = options(0);
    value.setup_timeout = Duration::ZERO;
    bad_options.push(value);
    let mut value = options(0);
    value.frame_timeout = Duration::ZERO;
    bad_options.push(value);
    let mut value = options(0);
    value.frame_timeout = Duration::from_secs(121);
    bad_options.push(value);
    let mut value = options(0);
    value.drain_timeout = Duration::ZERO;
    bad_options.push(value);
    let mut value = options(0);
    value.finish_timeout = Duration::ZERO;
    bad_options.push(value);
    let mut value = options(0);
    value.queue_capacity = 0;
    bad_options.push(value);
    let mut value = options(0);
    value.preroll_ns = -1;
    bad_options.push(value);
    let mut value = options(0);
    value.start_policy.min_remaining_ns = 0;
    bad_options.push(value);
    for settings in bad_options {
        assert!(
            NativeRoomNetwork::spawn_with(
                IDENTITY,
                &players,
                settings,
                |_, _, _| -> io::Result<RoomPlayIo<Stream>> {
                    panic!("invalid settings acquired a connector")
                }
            )
            .is_err()
        );
    }
    for players in [
        vec![],
        vec![PlayerId(0)],
        vec![PlayerId(7), PlayerId(7)],
        (1..=65).map(PlayerId).collect(),
    ] {
        assert!(
            NativeRoomNetwork::spawn_with(
                IDENTITY,
                &players,
                options(0),
                |_, _, _| -> io::Result<RoomPlayIo<Stream>> {
                    panic!("invalid roster acquired a connector")
                }
            )
            .is_err()
        );
    }
    for identity in [vec![], vec![1; 65_537]] {
        assert!(
            NativeRoomNetwork::spawn_with(
                &identity,
                &players,
                options(0),
                |_, _, _| -> io::Result<RoomPlayIo<Stream>> {
                    panic!("invalid identity acquired a connector")
                }
            )
            .is_err()
        );
    }
}

#[test]
fn local_command_refusals_and_partial_writes_preserve_healthy_actor_and_actual_roster_snapshots() {
    let mut client = endpoint(&[PlayerId(u32::MAX)], 0);
    client.script.lock().unwrap().writes.extend([
        Action::Limit(3),
        Action::Error(io::ErrorKind::WouldBlock),
        Action::Limit(usize::MAX),
    ]);
    assert!(step(&mut client, 0));
    assert_eq!(client.script.lock().unwrap().output, b"BKM");
    for command in [
        NativeRoomCommand::Seal,
        NativeRoomCommand::Ready,
        NativeRoomCommand::Drain,
        NativeRoomCommand::Publish {
            members: progress(&[PlayerId(u32::MAX)], 1),
            final_prefix: false,
        },
    ] {
        let before = client.actor.snapshot().clone();
        assert_eq!(
            client.actor.command(command, 0).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(*client.actor.snapshot(), before);
    }
    assert!(!step(&mut client, 0));
    assert!(step(&mut client, 0));
    assert_eq!(
        emitted(&client.script),
        RoomMessage::Join {
            identity: IDENTITY.to_vec(),
            players: vec![PlayerId(u32::MAX)]
        }
    );
    let outcome = client.actor.finish(None, true);
    assert!(outcome.cancelled);
    assert_eq!(outcome.receipts, NativeRoomReceipts::default());
    assert!(outcome.error.is_none() && outcome.cleanup_error.is_none());
    assert_eq!(client.script.lock().unwrap().finish_calls, 1);
    assert_eq!(client.script.lock().unwrap().drops, 1);

    for count in [2, 3, 4] {
        let mut cohort = collecting(count);
        let client = &mut cohort.clients[0];
        let room = client.actor.snapshot().room.as_ref().unwrap().clone();
        assert_eq!(room.phase, GroupRoomPhase::Collecting);
        assert_eq!(room.members, cohort.registry.room("room").unwrap().members);
        assert_eq!(room.members[0].players[0], room.members[1].players[0]);
        assert_ne!(room.members[0].id, room.members[1].id);
        let revision = client.actor.snapshot().revision;
        for at in [10, 11, 12] {
            assert!(!step(client, at));
            assert!(Arc::ptr_eq(
                &room,
                client.actor.snapshot().room.as_ref().unwrap()
            ));
            assert_eq!(client.actor.snapshot().revision, revision);
        }
        assert!(client.actor.snapshot().schedule.is_none());
        assert_eq!(
            client
                .actor
                .command(NativeRoomCommand::Ready, 12)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        for (index, mut client) in cohort.clients.into_iter().enumerate() {
            if index == 0 {
                let reads = client.script.lock().unwrap().read_calls;
                client.actor.command(NativeRoomCommand::Leave, 13).unwrap();
                client
                    .script
                    .lock()
                    .unwrap()
                    .reads
                    .push_back(Action::Limit(0));
                assert!(step(&mut client, 13));
                assert_eq!(emitted(&client.script), RoomMessage::Leave);
                assert_eq!(
                    client.script.lock().unwrap().read_calls,
                    reads,
                    "actual Leave completion skips subsequent EOF"
                );
                let result = client.actor.finish(None, true);
                assert!(result.leave_written && result.cancelled && result.cleanup_error.is_none());
                assert!(!result.receipts.drain_complete);
            } else {
                assert!(client.actor.finish(None, true).cleanup_error.is_none());
            }
        }
    }
}

#[test]
fn real_clock_and_all_accept_exchange_is_the_only_native_schedule_authority() {
    for count in [2, 3, 4] {
        let (mut cohort, target) = committed(count);
        for (index, client) in cohort.clients.iter_mut().enumerate() {
            let state = client.actor.snapshot();
            assert_eq!(state.participant, Some(cohort.ids[index]));
            assert_eq!(state.room.as_ref().unwrap().phase, GroupRoomPhase::Prepared);
            let schedule = state.schedule.unwrap();
            // Original samples bound server-minus-client to [-16-shift,7-shift].
            assert_eq!(schedule.song_target_ns, target + offset(index) + 4);
            assert_eq!(
                schedule.target_ns,
                target + offset(index) + 4 - index as i64 * 100
            );
            assert_eq!(schedule.uncertainty_ns, 23);
            assert_eq!(state.receipts, NativeRoomReceipts::default());
            let roster = state.room.as_ref().unwrap().clone();
            for _ in 0..3 {
                assert!(!step(client, 12_000 + offset(index)));
                assert_eq!(client.actor.snapshot().schedule, Some(schedule));
                assert!(Arc::ptr_eq(
                    &roster,
                    client.actor.snapshot().room.as_ref().unwrap()
                ));
            }
        }
        for client in cohort.clients {
            let result = client.actor.finish(None, true);
            assert!(result.cancelled);
        }
    }
}

#[test]
fn genuine_final_ack_and_explicit_drain_keep_distinct_receipts_until_actual_complete() {
    for count in [2, 3, 4] {
        let (mut cohort, mut relay, at) = locally_complete(count);
        for (index, client) in cohort.clients.iter_mut().enumerate() {
            let state = client.actor.snapshot();
            assert!(
                state.receipts.local_final_written
                    && state.receipts.local_final_acknowledged
                    && state.receipts.progress_complete
            );
            assert!(!state.receipts.drain_complete && !client.actor.finished());
            assert_eq!(state.peers.len(), count - 1);
            for (participant, prefix) in &state.peers {
                assert_ne!(*participant, cohort.ids[index]);
                assert!(prefix.final_prefix);
                assert_eq!(
                    prefix.members,
                    progress(
                        &cohort
                            .registry
                            .room("room")
                            .unwrap()
                            .members
                            .iter()
                            .find(|member| member.id == *participant)
                            .unwrap()
                            .players,
                        u64::MAX
                    )
                );
            }
            let retained = state.peers.clone();
            client
                .actor
                .command(NativeRoomCommand::Drain, at + offset(index))
                .unwrap();
            assert!(!client.actor.snapshot().receipts.drain_complete);
            assert_eq!(
                client
                    .actor
                    .command(NativeRoomCommand::Drain, at + offset(index))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
            step(client, at + offset(index));
            let ready = emitted(&client.script);
            assert_eq!(
                ready,
                RoomMessage::DrainReady {
                    participant: cohort.ids[index],
                    sequence: 1
                }
            );
            relay
                .receive_at(cohort.ids[index], &ready, at + 10)
                .unwrap();
            for ((id, prefix), (old_id, old_prefix)) in
                client.actor.snapshot().peers.iter().zip(&retained)
            {
                assert_eq!(id, old_id);
                assert!(Arc::ptr_eq(prefix, old_prefix));
            }
            if index + 1 < count {
                assert!(
                    relay
                        .poll_write_at(cohort.ids[0], at + 10)
                        .unwrap()
                        .is_none()
                );
            }
        }
        for index in 0..count {
            let frame = relay
                .poll_write_at(cohort.ids[index], at + 20)
                .unwrap()
                .unwrap();
            assert_eq!(
                decode_message(&frame.bytes).unwrap(),
                RoomMessage::DrainComplete {
                    participant: cohort.ids[index],
                    sequence: 1
                }
            );
            let client = &mut cohort.clients[index];
            deliver(
                client,
                &decode_message(&frame.bytes).unwrap(),
                at + offset(index) + 30,
            );
            relay.written(cohort.ids[index], frame.id).unwrap();
            assert!(client.actor.finished());
            assert!(client.actor.snapshot().receipts.drain_complete);
            assert!(
                !client
                    .actor
                    .drive(|| panic!("a completed room may not observe a fresh clock"))
                    .unwrap()
            );
            let counts = {
                let script = client.script.lock().unwrap();
                (script.read_calls, script.write_calls)
            };
            for _ in 0..3 {
                assert!(!step(client, at + offset(index) + 40));
            }
            let script = client.script.lock().unwrap();
            assert_eq!((script.read_calls, script.write_calls), counts);
            assert!(script.output.is_empty(), "Complete never fabricates Leave");
        }
        assert!(relay.drained());
        for client in cohort.clients {
            let result = client.actor.finish(None, false);
            assert!(!result.cancelled && result.error.is_none() && result.cleanup_error.is_none());
            assert!(result.receipts.drain_complete);
            assert!(!result.leave_written);
            assert_eq!(client.script.lock().unwrap().finish_calls, 1);
            assert_eq!(client.script.lock().unwrap().drops, 1);
        }
    }
}

#[test]
fn setup_and_drain_deadlines_are_fixed_and_cleanup_failure_cannot_replace_original_evidence() {
    let mut waiting = endpoint(&[PlayerId(7)], 0);
    step(&mut waiting, 0);
    emitted(&waiting.script);
    assert!(!step(&mut waiting, 999_999_999));
    let error = waiting.actor.drive(|| Ok(1_000_000_000)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    waiting.script.lock().unwrap().cleanup_error = true;
    let result = waiting.actor.finish(
        Some(NativeRoomFailure {
            kind: error.kind(),
            message: error.to_string(),
        }),
        false,
    );
    assert_eq!(result.error.unwrap().kind, io::ErrorKind::TimedOut);
    assert_eq!(
        result.cleanup_error.unwrap().kind,
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(result.receipts, NativeRoomReceipts::default());
    assert_eq!(waiting.script.lock().unwrap().finish_calls, 1);
    assert_eq!(waiting.script.lock().unwrap().drops, 1);

    // The production run loop receives the acquisition's original origin. A
    // late connector result cannot restart setup time or write an initial Join.
    let late = endpoint(&[PlayerId(7)], 0);
    let origin = Instant::now()
        .checked_sub(options(0).setup_timeout)
        .unwrap();
    let (_sender, receiver) = mpsc::sync_channel(2);
    let shared = Mutex::new(Shared {
        snapshot: NativeRoomSnapshot::default(),
        replies: VecDeque::new(),
        outstanding: 0,
    });
    run(
        late.actor,
        receiver,
        &shared,
        &AtomicBool::new(false),
        origin,
    );
    let outcome = shared.lock().unwrap().snapshot.terminal.clone().unwrap();
    assert_eq!(outcome.error.unwrap().kind, io::ErrorKind::TimedOut);
    assert!(!outcome.cancelled && outcome.cleanup_error.is_none());
    let script = late.script.lock().unwrap();
    assert_eq!(
        (
            script.read_calls,
            script.write_calls,
            script.finish_calls,
            script.drops
        ),
        (0, 0, 1, 1)
    );
    drop(script);

    let mut crossed = endpoint(&[PlayerId(7)], 0);
    let mut times = [999_999_999, 999_999_999, 1_000_000_000, 1_000_000_000].into_iter();
    let error = crossed
        .actor
        .drive(|| Ok(times.next().expect("extra setup clock sample")))
        .unwrap_err();
    assert!(times.next().is_none());
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(
        matches!(emitted(&crossed.script), RoomMessage::Join { .. }),
        "the actual write is retained despite the late processing refusal"
    );
    let result = crossed.actor.finish(Some(error.into()), false);
    assert_eq!(result.error.unwrap().kind, io::ErrorKind::TimedOut);
    assert!(result.cleanup_error.is_none());

    let (mut cohort, _) = committed(2);
    let client = &mut cohort.clients[0];
    let start = 20_000;
    client
        .actor
        .command(NativeRoomCommand::Drain, start)
        .unwrap();
    assert!(!step(client, start));
    assert!(client.script.lock().unwrap().output.is_empty());
    assert_eq!(
        client
            .actor
            .command(NativeRoomCommand::Drain, start + 999_999)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert!(!step(client, start + 999_999));
    let error = client.actor.drive(|| Ok(start + 1_000_000)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(
        !client.actor.snapshot().receipts.progress_complete
            && !client.actor.snapshot().receipts.drain_complete
    );
    for client in cohort.clients {
        client.actor.finish(None, true);
    }

    let (mut cohort, mut relay, at) = locally_complete(2);
    for index in 0..2 {
        let client = &mut cohort.clients[index];
        client
            .actor
            .command(NativeRoomCommand::Drain, at + offset(index))
            .unwrap();
        step(client, at + offset(index));
        let ready = emitted(&client.script);
        relay
            .receive_at(cohort.ids[index], &ready, at + 10)
            .unwrap();
    }
    let frame = relay
        .poll_write_at(cohort.ids[0], at + 20)
        .unwrap()
        .unwrap();
    assert!(matches!(
        decode_message(&frame.bytes).unwrap(),
        RoomMessage::DrainComplete { .. }
    ));
    let client = &mut cohort.clients[0];
    queued(&client.script, &decode_message(&frame.bytes).unwrap());
    assert!(step(client, at + offset(0) + 30)); // Only the actual eleven-byte header.
    assert!(!client.actor.snapshot().receipts.drain_complete);
    let deadline = at + offset(0) + 1_000_000;
    let mut times = [deadline - 1, deadline - 1, deadline - 1, deadline].into_iter();
    let error = client
        .actor
        .drive(|| Ok(times.next().expect("extra drain clock sample")))
        .unwrap_err();
    assert!(times.next().is_none());
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(
        client.actor.snapshot().receipts.drain_complete,
        "real Complete evidence survives its independently late processing outcome"
    );
    let failure = NativeRoomFailure::from(error);
    relay.written(cohort.ids[0], frame.id).unwrap();
    for (index, mut client) in cohort.clients.into_iter().enumerate() {
        let operation = if index == 0 {
            failure.clone()
        } else {
            client
                .script
                .lock()
                .unwrap()
                .reads
                .push_back(Action::Limit(0));
            let error = client
                .actor
                .drive(|| Ok(at + offset(index) + 40))
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
            assert!(
                client.actor.snapshot().receipts.local_final_written
                    && client.actor.snapshot().receipts.local_final_acknowledged
                    && client.actor.snapshot().receipts.progress_complete,
                "fatal common Stop cannot erase already observed receipt evidence"
            );
            error.into()
        };
        let result = client.actor.finish(Some(operation.clone()), false);
        assert_eq!(result.error, Some(operation));
        assert_eq!(result.receipts.drain_complete, index == 0);
        if index == 0 {
            assert_eq!(result.error, Some(failure.clone()));
        }
        assert!(result.cleanup_error.is_none());
    }
}

#[test]
fn bounded_front_queue_and_cancelled_late_acquisition_join_exactly_once_before_replacement() {
    use std::sync::mpsc;
    let (entered, enter_rx) = mpsc::sync_channel(1);
    let (release, release_rx) = mpsc::sync_channel(1);
    let script = Arc::new(Mutex::new(Script::default()));
    let connector_script = script.clone();
    let mut settings = options(0);
    settings.setup_timeout = Duration::from_secs(5);
    let mut owner = NativeRoomNetwork::spawn_with(
        IDENTITY,
        &[PlayerId(u32::MAX)],
        settings,
        move |session, cancelled, deadline| {
            entered.send(deadline).unwrap();
            release_rx
                .recv_timeout(Duration::from_secs(5))
                .map_err(|error| io::Error::new(io::ErrorKind::TimedOut, error))?;
            assert!(cancelled.load(std::sync::atomic::Ordering::Acquire));
            Ok(RoomPlayIo::new(session, Stream(connector_script)))
        },
    )
    .unwrap();
    let deadline = enter_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        deadline.duration_since(owner.origin),
        Duration::from_secs(5)
    );
    let first_clock = owner.clock_now_ns().unwrap();
    assert!(first_clock >= 0 && owner.clock_now_ns().unwrap() >= first_clock);
    let first = owner.try_command(NativeRoomCommand::Ready).unwrap();
    let second = owner.try_command(NativeRoomCommand::Seal).unwrap();
    assert!(first > 0 && second > first);
    assert_eq!(
        owner
            .try_command(NativeRoomCommand::Ready)
            .unwrap_err()
            .kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(script.lock().unwrap().read_calls, 0);
    assert_eq!(script.lock().unwrap().write_calls, 0);
    owner.request_stop();
    let (joined, join_rx) = mpsc::sync_channel(1);
    let joiner = std::thread::spawn(move || {
        let result = owner.stop();
        assert_eq!(
            owner.stop(),
            result,
            "repeated Stop returns retained joined ownership evidence"
        );
        let poll = owner.poll().unwrap();
        assert_eq!(poll.snapshot.terminal, Some(result.clone()));
        assert_eq!(
            poll.replies
                .iter()
                .map(|reply| reply.id)
                .collect::<Vec<_>>(),
            [first, second]
        );
        assert!(poll.replies.iter().all(|reply| reply.result.as_ref().unwrap_err().kind == io::ErrorKind::NotConnected));
        assert!(owner.poll().unwrap().replies.is_empty());
        joined.send(result).unwrap();
    });
    assert!(matches!(join_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert_eq!(
        script.lock().unwrap().finish_calls,
        0,
        "the connector still owns the pending acquisition"
    );
    release.send(()).unwrap();
    let result = join_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    joiner.join().unwrap();
    assert!(result.cancelled);
    assert!(!result.receipts.drain_complete && !result.leave_written);
    assert!(result.cleanup_error.is_none());
    let counts = {
        let script = script.lock().unwrap();
        assert_eq!(script.finish_calls, 1);
        assert_eq!(script.drops, 1);
        (script.read_calls, script.write_calls)
    };
    assert_eq!(
        counts,
        (0, 0),
        "a late cancelled stream cannot publish Join or fabricate completion"
    );
    let replacement = endpoint(&[PlayerId(91)], 0);
    assert_eq!(script.lock().unwrap().drops, 1);
    let outcome = replacement.actor.finish(None, true);
    assert!(outcome.cancelled && outcome.cleanup_error.is_none());
}
