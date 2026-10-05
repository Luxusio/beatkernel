//! Deferred common-driver fixtures. Scripted streams are not live transport or
//! output-clock acceptance; every room/clock/start transition uses real owners.
use crate::{
    local_players::PlayerId,
    multiplayer_group::MemberProgress,
    multiplayer_group_rooms::{
        GroupRoomMember, GroupRoomPhase, GroupRoomPolicy, GroupRoomRegistry,
    },
    multiplayer_room_clock::RoomClockExchange,
    multiplayer_room_io::RoomPlayIo,
    multiplayer_room_play::RoomPlayClient,
    multiplayer_room_progress::RoomProgressRelay,
    multiplayer_protocol::Progress,
    multiplayer_room_start::RoomStartCoordinator,
    multiplayer_room_wire::{RoomMessage, decode_message, encode_message},
    multiplayer_rooms::ParticipantId,
    multiplayer_start::{StartMessage, StartPolicy},
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    io::{self, Read, Write},
    rc::Rc,
};

const IDENTITY: &[u8] = &[0, 255, 17];
const PLAYERS: &[PlayerId] = &[PlayerId(u32::MAX), PlayerId(0x0102_0304)];
const JOIN: &[u8] = &[
    b'B', b'K', b'M', b'R', 2, 0, 1, 16, 0, 0, 0, 3, 0, 0, 0, 0, 255, 17, 2, 255, 255, 255, 255, 4,
    3, 2, 1,
];

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
    Overcount,
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
    drops: usize,
}
struct Stream(Rc<RefCell<Script>>);
impl Drop for Stream {
    fn drop(&mut self) {
        self.0.borrow_mut().drops += 1;
    }
}
impl Read for Stream {
    fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
        let mut s = self.0.borrow_mut();
        s.read_calls += 1;
        s.read_extents.push(target.len());
        match s.reads.pop_front().unwrap_or(Action::Limit(usize::MAX)) {
            Action::Error(kind) => Err(io::Error::new(kind, "scripted read")),
            Action::Overcount => Ok(target.len() + 1),
            Action::Limit(0) => Ok(0),
            Action::Limit(limit) => {
                if s.input.is_empty() {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                let count = limit.min(target.len()).min(s.input.len());
                for value in &mut target[..count] {
                    *value = s.input.pop_front().unwrap();
                }
                Ok(count)
            }
        }
    }
}
impl Write for Stream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut s = self.0.borrow_mut();
        s.write_calls += 1;
        match s.writes.pop_front().unwrap_or(Action::Limit(usize::MAX)) {
            Action::Error(kind) => Err(io::Error::new(kind, "scripted write")),
            Action::Overcount => Ok(bytes.len() + 1),
            Action::Limit(limit) => {
                let count = limit.min(bytes.len());
                s.output.extend_from_slice(&bytes[..count]);
                Ok(count)
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        panic!("a bounded room step must not flush or wait")
    }
}

struct Endpoint {
    io: RoomPlayIo<Stream>,
    script: Rc<RefCell<Script>>,
}
fn endpoint(players: &[PlayerId], preroll: i64) -> Endpoint {
    let script = Rc::new(RefCell::new(Script::default()));
    Endpoint {
        io: RoomPlayIo::new(
            RoomPlayClient::new(IDENTITY, players, policy(), preroll).unwrap(),
            Stream(script.clone()),
        ),
        script,
    }
}
fn step(endpoint: &mut Endpoint, now: i64) -> bool {
    let before = {
        let s = endpoint.script.borrow();
        (s.read_calls, s.write_calls)
    };
    let moved = endpoint.io.step(|| Ok(now)).unwrap();
    let s = endpoint.script.borrow();
    assert!(s.read_calls - before.0 <= 1);
    assert!(s.write_calls - before.1 <= 1);
    assert!(
        s.read_extents
            .iter()
            .all(|extent| (1..=4096).contains(extent))
    );
    moved
}
fn timed_step(endpoint: &mut Endpoint, times: &[i64]) -> bool {
    let mut values = times.iter().copied();
    let moved = endpoint
        .io
        .step(|| Ok(values.next().expect("unexpected clock acquisition")))
        .unwrap();
    assert_eq!(
        values.next(),
        None,
        "completion and processing each use their original observation"
    );
    moved
}
fn queue(endpoint: &Endpoint, message: &RoomMessage) {
    endpoint
        .script
        .borrow_mut()
        .input
        .extend(encode_message(message).unwrap());
}
fn deliver(endpoint: &mut Endpoint, message: &RoomMessage, now: i64) {
    queue(endpoint, message);
    // Header then bounded body reads, never an unbounded clock-spin driver.
    for _ in 0..32 {
        if endpoint.script.borrow().input.is_empty() {
            return;
        }
        assert!(step(endpoint, now));
    }
    panic!("bounded fixture frame did not drain");
}
fn emitted(endpoint: &Endpoint) -> RoomMessage {
    let bytes = std::mem::take(&mut endpoint.script.borrow_mut().output);
    decode_message(&bytes).unwrap()
}
fn registry_snapshot(registry: &GroupRoomRegistry) -> RoomMessage {
    let room = registry.room("room").unwrap();
    RoomMessage::Snapshot {
        members: room.members.to_vec(),
        phase: room.phase,
        deadline_ns: room.deadline_ns,
    }
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
            players: actual_players,
        } = emitted(&client)
        else {
            panic!("Join required")
        };
        assert_eq!(actual_players, players);
        let id = cohort
            .registry
            .join("room", &identity, &actual_players, 0)
            .unwrap()
            .id;
        deliver(&mut client, &RoomMessage::Admitted { participant: id }, 0);
        cohort.ids.push(id);
        cohort.clients.push(client);
        let snapshot = registry_snapshot(&cohort.registry);
        for client in &mut cohort.clients {
            deliver(client, &snapshot, 0);
        }
    }
    cohort
}
fn prepared(count: usize) -> Cohort {
    let mut cohort = collecting(count);
    cohort.clients[0].io.request_seal().unwrap();
    step(&mut cohort.clients[0], 1);
    assert_eq!(emitted(&cohort.clients[0]), RoomMessage::Seal);
    cohort.registry.seal(cohort.ids[0], 1).unwrap();
    let snapshot = registry_snapshot(&cohort.registry);
    for client in &mut cohort.clients {
        deliver(client, &snapshot, 1);
    }
    for index in 0..count {
        cohort.clients[index].io.request_ready().unwrap();
        step(&mut cohort.clients[index], 2);
        assert_eq!(emitted(&cohort.clients[index]), RoomMessage::Ready);
        cohort.registry.ready(cohort.ids[index], 2).unwrap();
        let snapshot = registry_snapshot(&cohort.registry);
        for client in &mut cohort.clients {
            deliver(client, &snapshot, 2);
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
            let client_ping = emitted(client);
            assert_eq!(
                client_ping,
                RoomMessage::ClockPing {
                    sequence,
                    sent_ns: at + shift
                }
            );
            clocks[index].written(ping.id, at + 2).unwrap();
            clocks[index].receive(&client_ping, at + 7).unwrap();
            deliver(
                client,
                &decode_message(&ping.bytes).unwrap(),
                at + shift + 11,
            );
            let pong = clocks[index].next(at + 13).unwrap().unwrap();
            clocks[index].written(pong.id, at + 14).unwrap();
            step(client, at + shift + 17);
            let reply = emitted(client);
            assert_eq!(
                reply,
                RoomMessage::ClockPong {
                    sequence,
                    sent_ns: at,
                    received_ns: at + shift + 11,
                    replied_ns: at + shift + 17
                }
            );
            clocks[index].receive(&reply, at + 23).unwrap();
            deliver(
                client,
                &decode_message(&pong.bytes).unwrap(),
                at + shift + 29,
            );
            assert_eq!(client.io.take_schedule().unwrap(), None);
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
        let RoomMessage::Start(ready) = emitted(&cohort.clients[index]) else {
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
        let RoomMessage::Start(accept) = emitted(&cohort.clients[index]) else {
            panic!("actual Accept required")
        };
        assert_eq!(accept, StartMessage::Accept(target));
        assert_eq!(cohort.clients[index].io.take_schedule().unwrap(), None);
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

fn fenced(endpoint: &mut Endpoint, failure: io::Error) {
    let before = {
        let s = endpoint.script.borrow();
        (s.read_calls, s.write_calls, s.output.clone())
    };
    let again = endpoint
        .io
        .step(|| panic!("a fenced driver cannot acquire a clock"))
        .unwrap_err();
    assert_eq!(
        (again.kind(), again.to_string()),
        (failure.kind(), failure.to_string())
    );
    for error in [
        endpoint.io.request_seal().unwrap_err(),
        endpoint.io.request_ready().unwrap_err(),
        endpoint.io.request_leave().unwrap_err(),
        endpoint.io.take_schedule().unwrap_err(),
    ] {
        assert_eq!(
            (error.kind(), error.to_string()),
            (failure.kind(), failure.to_string())
        );
    }
    endpoint.io.stop();
    endpoint.io.stop();
    assert_eq!(
        endpoint.io.take_schedule().unwrap_err().to_string(),
        failure.to_string()
    );
    let s = endpoint.script.borrow();
    assert_eq!(
        (s.read_calls, s.write_calls, &s.output),
        (before.0, before.1, &before.2)
    );
    assert_eq!(s.drops, 0, "transport disposal belongs to the caller");
}

#[test]
fn literal_join_partial_writes_and_coalesced_reads_retain_one_real_receipt_boundary() {
    let mut endpoint = endpoint(PLAYERS, 100);
    endpoint.script.borrow_mut().writes.extend([
        Action::Limit(5),
        Action::Error(io::ErrorKind::WouldBlock),
        Action::Error(io::ErrorKind::Interrupted),
        Action::Limit(usize::MAX),
    ]);
    assert!(timed_step(&mut endpoint, &[20, 21]));
    assert_eq!(endpoint.script.borrow().output, JOIN[..5]);
    let retained = endpoint.io.session().clone();
    assert_eq!(
        endpoint.io.request_ready().unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        endpoint.io.request_leave().unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(endpoint.io.session(), &retained);
    assert!(!timed_step(&mut endpoint, &[22]));
    assert!(!timed_step(&mut endpoint, &[23]));
    assert_eq!(endpoint.script.borrow().output, JOIN[..5]);
    assert!(timed_step(&mut endpoint, &[24, 25, 26]));
    assert_eq!(endpoint.script.borrow().output, JOIN);
    assert_eq!(endpoint.io.session().participant(), None);
    let id = ParticipantId(0x8877_6655_4433_2211);
    queue(&endpoint, &RoomMessage::Admitted { participant: id });
    queue(
        &endpoint,
        &RoomMessage::Snapshot {
            members: vec![GroupRoomMember {
                id,
                players: PLAYERS.to_vec(),
                prepared: false,
            }],
            phase: GroupRoomPhase::Collecting,
            deadline_ns: Some(i64::MAX),
        },
    );
    endpoint
        .script
        .borrow_mut()
        .reads
        .extend([Action::Limit(3), Action::Error(io::ErrorKind::Interrupted)]);
    assert!(timed_step(&mut endpoint, &[27, 28]));
    assert!(!timed_step(&mut endpoint, &[29]));
    assert!(timed_step(&mut endpoint, &[30, 31])); // Complete header only.
    assert_eq!(endpoint.io.session().participant(), None);
    assert!(timed_step(&mut endpoint, &[32, 33, 34]));
    assert_eq!(endpoint.io.session().participant(), Some(id));
    assert!(
        endpoint.io.session().room().is_none(),
        "coalesced next frame has not been read yet"
    );
    assert!(step(&mut endpoint, 35));
    assert!(step(&mut endpoint, 36));
    assert_eq!(
        endpoint.io.session().room().unwrap().members[0].players,
        PLAYERS
    );
    assert_eq!(
        endpoint.script.borrow().output,
        JOIN,
        "no interrupted prefix is resent"
    );
    let mut members = endpoint.io.session().room().unwrap().members.to_vec();
    for host in 1..64 {
        members.push(GroupRoomMember {
            id: ParticipantId(host),
            prepared: false,
            players: (0..64).map(|slot| PlayerId(u32::MAX - slot)).collect(),
        });
    }
    deliver(
        &mut endpoint,
        &RoomMessage::Snapshot {
            members,
            phase: GroupRoomPhase::Collecting,
            deadline_ns: Some(i64::MAX),
        },
        37,
    );
    assert_eq!(endpoint.io.session().room().unwrap().members.len(), 64);
    assert_eq!(
        endpoint.io.session().room().unwrap().members[63]
            .players
            .len(),
        64
    );
    assert!(
        endpoint.script.borrow().read_extents.contains(&4096),
        "large valid snapshots cross the bounded scratch capacity"
    );
    assert_eq!(endpoint.io.take_schedule().unwrap(), None);
}

#[test]
fn two_three_four_actual_cohorts_complete_clock_and_all_accept_barriers_before_once_only_schedules()
{
    for count in [2usize, 3, 4] {
        let (mut cohort, target) = committed(count);
        for (index, client) in cohort.clients.iter_mut().enumerate() {
            assert_eq!(client.io.session().participant(), Some(cohort.ids[index]));
            assert_eq!(client.io.session().room().unwrap().members.len(), count);
            let schedule = client.io.take_schedule().unwrap().unwrap();
            // Original clock samples bound server-minus-client to [-16-shift, 7-shift].
            // The existing common midpoint projects to target+shift+4, with width23.
            assert_eq!(schedule.song_target_ns, target + offset(index) + 4);
            assert_eq!(
                schedule.target_ns,
                target + offset(index) + 4 - index as i64 * 100
            );
            assert_eq!(schedule.uncertainty_ns, 23);
            assert_eq!(client.io.take_schedule().unwrap(), None);
            assert!(!step(client, 11_300 + offset(index)));
            assert_eq!(client.io.take_schedule().unwrap(), None);
            assert!(client.script.borrow().output.is_empty());
        }
    }
}

#[test]
fn week_long_probe_fields_use_final_read_capture_and_poll_time_instead_of_later_processing() {
    let mut cohort = prepared(2);
    let mut server =
        RoomClockExchange::new(cohort.registry.room("room").unwrap(), cohort.ids[0]).unwrap();
    let client = &mut cohort.clients[0];
    let base = 604_800_000_000_000;
    assert!(timed_step(client, &[base + 100, base + 110, base + 120]));
    let client_ping = emitted(client);
    assert_eq!(
        client_ping,
        RoomMessage::ClockPing {
            sequence: 1,
            sent_ns: base + 100
        }
    );
    let server_ping = server.next(base + 500).unwrap().unwrap();
    server.written(server_ping.id, base + 510).unwrap();
    server.receive(&client_ping, base + 530).unwrap();
    queue(client, &decode_message(&server_ping.bytes).unwrap());
    assert!(timed_step(client, &[base + 125, base + 130])); // Header is not the message timestamp.
    client.script.borrow_mut().reads.push_back(Action::Limit(1));
    assert!(timed_step(client, &[base + 140, base + 150]));
    assert!(timed_step(client, &[base + 160, base + 170, base + 200]));
    assert!(client.script.borrow().output.is_empty());
    assert!(timed_step(client, &[base + 220, base + 230, base + 240]));
    let client_pong = emitted(client);
    assert_eq!(
        client_pong,
        RoomMessage::ClockPong {
            sequence: 1,
            sent_ns: base + 500,
            received_ns: base + 170,
            replied_ns: base + 220
        }
    );
    let server_pong = server.next(base + 540).unwrap().unwrap();
    server.written(server_pong.id, base + 550).unwrap();
    server.receive(&client_pong, base + 650).unwrap();
    queue(client, &decode_message(&server_pong.bytes).unwrap());
    assert!(timed_step(client, &[base + 245, base + 250]));
    assert!(timed_step(client, &[base + 255, base + 260, base + 300]));
    assert_eq!(
        server.estimate(),
        None,
        "one reply never substitutes for all eight exchanges"
    );
    assert_eq!(client.io.take_schedule().unwrap(), None);
}

#[test]
fn early_admission_controls_and_malformed_frames_fence_after_only_the_actual_received_prefix() {
    let mut early = endpoint(PLAYERS, 0);
    early
        .script
        .borrow_mut()
        .writes
        .extend([Action::Limit(1), Action::Error(io::ErrorKind::WouldBlock)]);
    queue(
        &early,
        &RoomMessage::Admitted {
            participant: ParticipantId(7),
        },
    );
    assert!(step(&mut early, 0)); // Partial Join plus the complete admission header.
    let error = early.io.step(|| Ok(1)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(early.script.borrow().output, JOIN[..1]);
    assert_eq!(early.io.session().participant(), None);
    fenced(&mut early, error);

    for message in [
        RoomMessage::ClockPing {
            sequence: 1,
            sent_ns: 0,
        },
        RoomMessage::Start(StartMessage::ClockReady(0)),
        RoomMessage::Start(StartMessage::Commit(50_000)),
    ] {
        let mut endpoint = endpoint(PLAYERS, 0);
        assert!(step(&mut endpoint, 0));
        queue(&endpoint, &message);
        assert!(step(&mut endpoint, 1));
        let error = endpoint.io.step(|| Ok(2)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(endpoint.script.borrow().output, JOIN);
        fenced(&mut endpoint, error);
    }
    for header in [
        [b'B', b'K', b'M', b'R', 1, 0, 2, 8, 0, 0, 0],
        [b'B', b'K', b'M', b'R', 2, 0, 2, 9, 0, 0, 0],
        [b'B', b'K', b'M', b'P', 2, 0, 2, 8, 0, 0, 0],
    ] {
        let mut endpoint = endpoint(PLAYERS, 0);
        endpoint.script.borrow_mut().input.extend(header);
        endpoint.script.borrow_mut().input.extend([0xaa; 8]);
        let error = endpoint.io.step(|| Ok(0)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            endpoint.script.borrow().input.len(),
            8,
            "invalid headers cannot acquire a body"
        );
        fenced(&mut endpoint, error);
    }
}

#[test]
fn transport_counts_eof_and_clock_failures_latch_without_future_io_clocks_or_schedule_access() {
    for (write, action, kind) in [
        (true, Action::Limit(0), io::ErrorKind::WriteZero),
        (true, Action::Overcount, io::ErrorKind::InvalidData),
        (false, Action::Overcount, io::ErrorKind::InvalidData),
        (false, Action::Limit(0), io::ErrorKind::UnexpectedEof),
        (
            true,
            Action::Error(io::ErrorKind::BrokenPipe),
            io::ErrorKind::BrokenPipe,
        ),
        (
            false,
            Action::Error(io::ErrorKind::ConnectionReset),
            io::ErrorKind::ConnectionReset,
        ),
    ] {
        let mut endpoint = endpoint(PLAYERS, 0);
        if write {
            endpoint.script.borrow_mut().writes.push_back(action);
        } else {
            endpoint.script.borrow_mut().reads.push_back(action);
        }
        let failure = endpoint
            .io
            .step(|| Ok(20 * 60 * 60 * 1_000_000_000))
            .unwrap_err();
        assert_eq!(failure.kind(), kind);
        fenced(&mut endpoint, failure);
    }
    for times in [vec![-1], vec![100, 99], vec![100, 110, 109]] {
        let mut endpoint = endpoint(PLAYERS, 0);
        let mut clock = times.into_iter();
        let failure = endpoint
            .io
            .step(|| Ok(clock.next().expect("extra clock call")))
            .unwrap_err();
        assert_eq!(failure.kind(), io::ErrorKind::InvalidData);
        assert_eq!(clock.next(), None);
        fenced(&mut endpoint, failure);
    }
    let mut partial = endpoint(PLAYERS, 0);
    partial
        .script
        .borrow_mut()
        .writes
        .push_back(Action::Limit(1));
    assert!(timed_step(&mut partial, &[100, 200]));
    let failure = partial.io.step(|| Ok(150)).unwrap_err();
    assert_eq!(
        partial.script.borrow().output,
        JOIN[..1],
        "partial completion advances the driver clock too"
    );
    fenced(&mut partial, failure);

    let mut failed_clock = endpoint(PLAYERS, 0);
    let mut calls = 0;
    let failure = failed_clock
        .io
        .step(|| {
            calls += 1;
            if calls == 2 {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "original clock failure",
                ))
            } else {
                Ok(0)
            }
        })
        .unwrap_err();
    assert_eq!(failure.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(
        failed_clock.script.borrow().output,
        JOIN,
        "real bytes moved before the failed observation"
    );
    assert_eq!(calls, 2);
    fenced(&mut failed_clock, failure);

    let mut maximum = endpoint(PLAYERS, 0);
    assert!(timed_step(&mut maximum, &[i64::MAX, i64::MAX, i64::MAX]));
    assert!(!timed_step(&mut maximum, &[i64::MAX]));
    maximum.io.stop();
    assert_eq!(
        maximum.io.take_schedule().unwrap_err().kind(),
        io::ErrorKind::NotConnected
    );
}

#[test]
fn stop_and_actual_leave_withhold_unconsumed_schedules_and_return_stream_cleanup_to_the_caller() {
    for leave in [false, true] {
        let (mut cohort, _) = committed(2);
        let mut client = cohort.clients.remove(0);
        assert_eq!(
            client.io.session().room().unwrap().phase,
            GroupRoomPhase::Prepared
        );
        if leave {
            client.io.request_leave().unwrap();
            assert_eq!(client.io.take_schedule().unwrap(), None);
            client
                .script
                .borrow_mut()
                .writes
                .push_back(Action::Limit(5));
            assert!(step(&mut client, 12_000));
            assert!(!client.io.session().leave_written());
            let reads = client.script.borrow().read_calls;
            client.script.borrow_mut().reads.push_back(Action::Limit(0));
            assert!(step(&mut client, 12_001));
            assert!(client.io.session().leave_written());
            assert_eq!(
                client.script.borrow().read_calls,
                reads,
                "a full Leave receipt skips the queued EOF read"
            );
            assert_eq!(emitted(&client), RoomMessage::Leave);
        } else {
            client.io.stop();
            client.io.stop();
        }
        let error = client.io.take_schedule().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotConnected);
        fenced(&mut client, error);
        let Endpoint { io, script } = client;
        let before = {
            let s = script.borrow();
            (s.read_calls, s.write_calls)
        };
        let stream = io.into_stream();
        assert_eq!(script.borrow().drops, 0);
        assert_eq!(
            (script.borrow().read_calls, script.borrow().write_calls),
            before
        );
        drop(stream);
        assert_eq!(script.borrow().drops, 1);
    }
}

fn completed_progress(count: usize) -> (Cohort, RoomProgressRelay, i64) {
    let (mut cohort, _) = committed(count);
    let room = cohort.registry.room("room").unwrap();
    let mut relay = RoomProgressRelay::new(room).unwrap();
    relay.activate().unwrap();
    for index in 0..count {
        let client = &mut cohort.clients[index];
        let before = client.io.session().clone();
        assert_eq!(
            client.io.request_drain().unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(*client.io.session(), before);
        let rows = room.members[index]
            .players
            .iter()
            .copied()
            .map(|player| MemberProgress {
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
        client.io.publish_progress(&rows, true).unwrap();
        assert!(step(client, 12_000 + offset(index)));
        let upload = emitted(client);
        assert!(matches!(upload, RoomMessage::Progress(_)));
        relay
            .receive_at(cohort.ids[index], &upload, 12_010)
            .unwrap();
        assert!(client.io.local_final_written());
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
                assert!(cohort.clients[index].script.borrow().output.is_empty());
                relay.written(cohort.ids[index], frame.id).unwrap();
            }
        }
        for index in 0..count {
            let client = &mut cohort.clients[index];
            step(client, at + offset(index) + 20);
            if !client.script.borrow().output.is_empty() {
                let ack = emitted(client);
                assert!(matches!(ack, RoomMessage::FinalAck { .. }));
                relay.receive_at(cohort.ids[index], &ack, at + 30).unwrap();
            }
        }
        if relay.complete()
            && cohort
                .clients
                .iter()
                .all(|client| client.io.progress_complete())
        {
            assert!(
                cohort
                    .clients
                    .iter()
                    .all(|client| !client.io.drain_complete())
            );
            return (cohort, relay, at + 100);
        }
    }
    panic!("bounded real driver final/ACK exchange did not complete");
}

#[test]
fn actual_stream_ready_prefixes_and_complete_fragments_finish_without_automatic_leave_or_further_io()
 {
    for count in [2usize, 3, 4] {
        let (mut cohort, mut relay, at) = completed_progress(count);
        for index in 0..count {
            let client = &mut cohort.clients[index];
            client.io.request_drain().unwrap();
            let requested = client.io.session().clone();
            assert_eq!(
                client.io.request_drain().unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert_eq!(*client.io.session(), requested);
            client.script.borrow_mut().writes.extend([
                Action::Limit(5),
                Action::Error(io::ErrorKind::WouldBlock),
                Action::Error(io::ErrorKind::Interrupted),
                Action::Limit(usize::MAX),
            ]);
            let local = at + offset(index);
            assert!(timed_step(client, &[local, local + 1]));
            assert_eq!(client.script.borrow().output.as_slice(), b"BKMR\x02");
            assert!(!timed_step(client, &[local + 2]));
            assert!(!timed_step(client, &[local + 3]));
            assert!(!client.io.drain_complete());
            assert!(timed_step(client, &[local + 4, local + 5, local + 6]));
            let ready = emitted(client);
            assert_eq!(
                ready,
                RoomMessage::DrainReady {
                    participant: cohort.ids[index],
                    sequence: 1,
                }
            );
            relay
                .receive_at(cohort.ids[index], &ready, at + 10)
                .unwrap();
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
                .poll_write_at(cohort.ids[index], at + 11)
                .unwrap()
                .unwrap();
            let complete = decode_message(&frame.bytes).unwrap();
            assert_eq!(
                complete,
                RoomMessage::DrainComplete {
                    participant: cohort.ids[index],
                    sequence: 1,
                }
            );
            let client = &mut cohort.clients[index];
            queue(client, &complete);
            client.script.borrow_mut().reads.extend([
                Action::Limit(3),
                Action::Error(io::ErrorKind::WouldBlock),
                Action::Limit(8),
                Action::Limit(1),
                Action::Error(io::ErrorKind::Interrupted),
                Action::Limit(usize::MAX),
                Action::Limit(0),
            ]);
            let local = at + offset(index) + 20;
            assert!(timed_step(client, &[local, local + 1]));
            assert!(!timed_step(client, &[local + 2]));
            assert!(timed_step(client, &[local + 3, local + 4]));
            assert!(timed_step(client, &[local + 5, local + 6]));
            assert!(!timed_step(client, &[local + 7]));
            assert!(!client.io.drain_complete());
            assert!(timed_step(client, &[local + 8, local + 9, local + 10]));
            assert!(client.io.drain_complete() && client.io.progress_complete());
            assert!(client.io.local_final_written() && client.io.local_final_acknowledged());
            assert!(!client.io.session().leave_written());
            let before = {
                let s = client.script.borrow();
                (s.read_calls, s.write_calls, s.output.clone(), s.reads.len())
            };
            for _ in 0..3 {
                assert!(
                    !client
                        .io
                        .step(|| panic!("completed drain must not acquire time"))
                        .unwrap()
                );
            }
            let s = client.script.borrow();
            assert_eq!(
                (s.read_calls, s.write_calls, &s.output, s.reads.len()),
                (before.0, before.1, &before.2, before.3)
            );
            assert_eq!(
                s.reads.len(),
                1,
                "the queued EOF remains unobserved after completion"
            );
            assert_eq!(s.drops, 0);
            drop(s);
            assert!(!relay.drained());
            relay.written(cohort.ids[index], frame.id).unwrap();
        }
        assert!(relay.drained());
        assert_eq!(cohort.registry.room("room").unwrap().members.len(), count);
        for client in cohort.clients {
            let Endpoint { io, script } = client;
            assert!(io.drain_complete());
            let stream = io.into_stream();
            assert_eq!(script.borrow().drops, 0);
            drop(stream);
            assert_eq!(
                script.borrow().drops,
                1,
                "only the caller disposes the stream"
            );
        }
    }
}

#[test]
fn completed_client_handoff_is_idle_immediately_while_stop_and_leave_cancel_partial_drain() {
    let (mut cohort, mut relay, at) = completed_progress(2);
    for index in 0..2 {
        let client = &mut cohort.clients[index];
        client.io.request_drain().unwrap();
        assert!(step(client, at + offset(index)));
        relay
            .receive_at(cohort.ids[index], &emitted(client), at + 1)
            .unwrap();
    }
    for index in 0..2 {
        let complete = relay
            .poll_write_at(cohort.ids[index], at + 2)
            .unwrap()
            .unwrap();
        deliver(
            &mut cohort.clients[index],
            &decode_message(&complete.bytes).unwrap(),
            at + offset(index) + 3,
        );
        relay.written(cohort.ids[index], complete.id).unwrap();
    }
    assert!(relay.drained());
    // This public handoff has no outstanding frame: the original owner reached
    // completion through actual Ready writes and real relay-generated notices.
    let completed = cohort.clients[0].io.session().clone();
    assert!(completed.drain_complete());
    let script = Rc::new(RefCell::new(Script::default()));
    script.borrow_mut().reads.push_back(Action::Limit(0));
    script
        .borrow_mut()
        .writes
        .push_back(Action::Error(io::ErrorKind::BrokenPipe));
    let mut handoff = RoomPlayIo::new(completed, Stream(script.clone()));
    assert!(
        !handoff
            .step(|| panic!("already completed client must be idle at construction"))
            .unwrap()
    );
    assert_eq!(
        (script.borrow().read_calls, script.borrow().write_calls),
        (0, 0)
    );
    assert!(handoff.drain_complete());
    assert_eq!(
        handoff.request_drain().unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    handoff.stop();
    assert!(!handoff.drain_complete() && !handoff.progress_complete());
    assert!(handoff.local_final_written() && handoff.local_final_acknowledged());
    assert_eq!(
        handoff
            .step(|| panic!("explicit stop must not acquire time"))
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotConnected
    );
    assert_eq!(
        (script.borrow().read_calls, script.borrow().write_calls),
        (0, 0)
    );
    let stream = handoff.into_stream();
    assert_eq!(script.borrow().drops, 0);
    drop(stream);
    assert_eq!(script.borrow().drops, 1);

    for leave in [false, true] {
        let (mut cohort, _, at) = completed_progress(2);
        let client = &mut cohort.clients[0];
        client.io.request_drain().unwrap();
        client
            .script
            .borrow_mut()
            .writes
            .push_back(Action::Limit(5));
        assert!(step(client, at + offset(0)));
        assert!(!client.io.drain_complete());
        if leave {
            client.io.request_leave().unwrap();
            assert_eq!(
                client.io.request_drain().unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert!(step(client, at + offset(0) + 1));
            assert_eq!(
                emitted(client),
                RoomMessage::DrainReady {
                    participant: cohort.ids[0],
                    sequence: 1,
                }
            );
            assert!(!client.io.drain_complete() && !client.io.progress_complete());
            let reads = client.script.borrow().read_calls;
            client.script.borrow_mut().reads.push_back(Action::Limit(0));
            assert!(step(client, at + offset(0) + 2));
            assert_eq!(emitted(client), RoomMessage::Leave);
            assert_eq!(client.script.borrow().read_calls, reads);
            assert!(client.io.session().leave_written());
        } else {
            client.io.stop();
            assert_eq!(client.script.borrow().output.as_slice(), b"BKMR\x02");
        }
        assert!(!client.io.drain_complete());
        assert!(client.io.local_final_written() && client.io.local_final_acknowledged());
        let error = client.io.request_drain().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotConnected);
        fenced(client, error);
    }
}

#[test]
fn partial_frame_fixed_timeout_refuses_before_more_io_without_acquiring_extra_clocks() {
    let mut endpoint = endpoint(PLAYERS, 0);
    assert!(endpoint.io.configure_frame_wait(0).is_err());
    assert!(endpoint.io.configure_frame_wait(1_000_000).is_ok());
    assert!(endpoint.io.configure_frame_wait(120_000_000_000).is_err());
    let mut clocks = 0;
    assert!(
        endpoint
            .io
            .step(|| {
                clocks += 1;
                Ok(0)
            })
            .unwrap()
    );
    assert_eq!(
        clocks, 3,
        "original Join poll/completion/processing observations only"
    );
    let RoomMessage::Join { identity, players } = emitted(&endpoint) else {
        panic!("Join")
    };
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 2, 8, 1_000).unwrap());
    let id = registry.join("room", &identity, &players, 0).unwrap().id;
    let bytes = encode_message(&RoomMessage::Admitted { participant: id }).unwrap();
    endpoint.script.borrow_mut().input.extend(&bytes[..11]);
    clocks = 0;
    assert!(
        endpoint
            .io
            .step(|| {
                clocks += 1;
                Ok(10)
            })
            .unwrap()
    );
    assert_eq!(
        clocks, 2,
        "partial read retains original poll/capture observations"
    );
    let calls = (
        endpoint.script.borrow().read_calls,
        endpoint.script.borrow().write_calls,
    );
    endpoint.script.borrow_mut().input.extend(&bytes[11..]);
    clocks = 0;
    let first = endpoint
        .io
        .step(|| {
            clocks += 1;
            Ok(1_000_010)
        })
        .unwrap_err();
    assert_eq!(first.kind(), io::ErrorKind::TimedOut);
    assert_eq!(clocks, 1);
    assert_eq!(
        (
            endpoint.script.borrow().read_calls,
            endpoint.script.borrow().write_calls
        ),
        calls
    );
    assert_eq!(endpoint.io.session().participant(), None);
    assert!(endpoint.io.session().room().is_none());
    assert!(
        endpoint
            .io
            .step(|| panic!("failed IO must not acquire another clock"))
            .is_err()
    );
}

#[test]
fn complete_frame_captured_before_expiry_but_processed_at_expiry_cannot_admit_participant() {
    let mut endpoint = endpoint(PLAYERS, 0);
    endpoint.io.configure_frame_wait(1_000_000).unwrap();
    step(&mut endpoint, 0);
    let RoomMessage::Join { identity, players } = emitted(&endpoint) else {
        panic!("Join")
    };
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 2, 8, 1_000).unwrap());
    let id = registry.join("room", &identity, &players, 0).unwrap().id;
    let bytes = encode_message(&RoomMessage::Admitted { participant: id }).unwrap();
    endpoint.script.borrow_mut().input.extend(&bytes[..11]);
    step(&mut endpoint, 10);
    endpoint.script.borrow_mut().input.extend(&bytes[11..]);
    let mut times = [1_000_009, 1_000_009, 1_000_010].into_iter();
    let error = endpoint
        .io
        .step(|| Ok(times.next().expect("no extra frame clock")))
        .unwrap_err();
    assert_eq!(times.next(), None);
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(endpoint.io.session().participant(), None);
    assert!(endpoint.io.session().room().is_none());
    assert!(
        endpoint.script.borrow().input.is_empty(),
        "actual late bytes remain read history"
    );
    assert!(endpoint.io.take_schedule().is_err());
}
