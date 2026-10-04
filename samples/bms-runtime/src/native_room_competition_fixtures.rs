//! Deferred controller fixtures. The port is scripted; start and drain evidence
//! below comes from the actual common owners, without sockets or native output.
use super::*;
use crate::{
    local_players::PlayerId,
    multiplayer_clock::{ClockFilter, ClockSample},
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_group_rooms::{GroupRoomPolicy, GroupRoomRegistry},
    multiplayer_protocol::Progress,
    multiplayer_room_progress::RoomProgressRelay,
    multiplayer_room_progress_client::RoomProgressClient,
    multiplayer_room_start::RoomStartCoordinator,
    multiplayer_room_wire::{RoomMessage, decode_message},
    multiplayer_start::{StartAgreement, StartPolicy, StartRole, StartSchedule},
    native_room_network::{NativeRoomReceipts, NativeRoomRoster},
    native_start::NativeStartAgreement,
};
use beatkernel::time::{ClockDomainId, ClockPoint, Timestamp};
use std::{cell::RefCell, collections::VecDeque, rc::Rc, sync::Arc};

fn policy() -> StartPolicy {
    StartPolicy {
        lead_ns: 10_000,
        min_remaining_ns: 1_000,
        max_age_ns: 100_000,
        max_uncertainty_ns: 100,
        max_release_lateness_ns: 25,
    }
}

fn prepared(hosts: usize, slots: usize) -> GroupRoomRegistry {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, hosts, 8, 100).unwrap());
    let players = (0..slots)
        .map(|slot| PlayerId(u32::MAX - slot as u32))
        .collect::<Vec<_>>();
    let ids = (0..hosts)
        .map(|_| {
            registry
                .join("room", b"actual\0identity", &players, 0)
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

fn snapshot(registry: &GroupRoomRegistry) -> NativeRoomSnapshot {
    let room = registry.room("room").unwrap();
    NativeRoomSnapshot {
        revision: 1,
        participant: Some(room.members[0].id),
        room: Some(Arc::new(NativeRoomRoster {
            members: room.members.to_vec(),
            phase: room.phase,
            deadline_ns: room.deadline_ns,
        })),
        ..NativeRoomSnapshot::default()
    }
}

fn rows(players: &[PlayerId], hits: u64) -> Vec<MemberProgress> {
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

// Real all-Accept/all-Commit exchange. The exact ClockSample bounds imply zero
// uncertainty; each client still retains its own genuine preroll subtraction.
fn schedules(registry: &GroupRoomRegistry) -> Vec<StartSchedule> {
    let room = registry.room("room").unwrap();
    let mut filter = ClockFilter::new();
    filter
        .observe(ClockSample::new(1_000, 1_000, 1_000, 1_000).unwrap())
        .unwrap();
    let estimate = filter.estimate().unwrap();
    let mut server = RoomStartCoordinator::new(room, policy()).unwrap();
    let mut joins = Vec::new();
    for (index, member) in room.members.iter().enumerate() {
        let mut join =
            StartAgreement::new_at(StartRole::Join, policy(), index as i64 * 100).unwrap();
        join.prepare(estimate).unwrap();
        server.prepare(member.id, estimate, 1_000).unwrap();
        let ready = server.next(member.id, 1_000).unwrap().unwrap();
        server.written(member.id, ready, 1_000).unwrap();
        join.receive(ready, 1_000).unwrap();
        let ready = join.next(1_000).unwrap().unwrap();
        join.written(ready, 1_000).unwrap();
        server.receive(member.id, ready, 1_000).unwrap();
        assert!(join.take_schedule().is_none());
        joins.push(join);
    }
    for (member, join) in room.members.iter().zip(&mut joins) {
        let proposal = server.next(member.id, 1_000).unwrap().unwrap();
        server.written(member.id, proposal, 1_000).unwrap();
        join.receive(proposal, 1_000).unwrap();
        let accept = join.next(1_000).unwrap().unwrap();
        join.written(accept, 1_000).unwrap();
        server.receive(member.id, accept, 1_000).unwrap();
        assert!(join.take_schedule().is_none());
    }
    for (member, join) in room.members.iter().zip(&mut joins) {
        let commit = server.next(member.id, 1_000).unwrap().unwrap();
        server.written(member.id, commit, 1_000).unwrap();
        join.receive(commit, 1_000).unwrap();
    }
    assert!(server.committed());
    joins
        .iter_mut()
        .map(|join| join.take_schedule().unwrap())
        .collect()
}

fn receipts(client: &RoomProgressClient) -> NativeRoomReceipts {
    NativeRoomReceipts {
        local_final_written: client.local_final_written(),
        local_final_acknowledged: client.local_final_acknowledged(),
        progress_complete: client.local_complete(),
        drain_complete: client.drain_complete(),
    }
}

// These are protocol-generated port snapshots, not booleans invented by the
// scripted port. Every frame is decoded and receives its actual full-write ID.
fn completion_evidence(registry: &GroupRoomRegistry) -> (NativeRoomReceipts, NativeRoomReceipts) {
    let room = registry.room("room").unwrap();
    let mut relay = RoomProgressRelay::new(room).unwrap();
    relay.activate().unwrap();
    let mut clients = room
        .members
        .iter()
        .map(|member| {
            let mut client = RoomProgressClient::new(room, member.id).unwrap();
            client.activate().unwrap();
            client.publish(&rows(&member.players, 7), true).unwrap();
            client
        })
        .collect::<Vec<_>>();
    for (member, client) in room.members.iter().zip(&mut clients) {
        let frame = client.poll_write(10).unwrap().unwrap();
        relay
            .receive_at(member.id, &decode_message(&frame.bytes).unwrap(), 10)
            .unwrap();
        client.written(frame.id).unwrap();
    }
    for turn in 0..room.members.len() + 2 {
        let now = 20 + turn as i64;
        for (member, client) in room.members.iter().zip(&mut clients) {
            if let Some(frame) = relay.poll_write_at(member.id, now).unwrap() {
                client
                    .receive(&decode_message(&frame.bytes).unwrap(), now)
                    .unwrap();
                relay.written(member.id, frame.id).unwrap();
            }
            if let Some(frame) = client.poll_write(now).unwrap() {
                relay
                    .receive_at(member.id, &decode_message(&frame.bytes).unwrap(), now)
                    .unwrap();
                client.written(frame.id).unwrap();
            }
        }
    }
    assert!(relay.complete());
    assert!(clients.iter().all(RoomProgressClient::local_complete));
    let local = receipts(&clients[0]);
    assert!(!local.drain_complete);
    for (member, client) in room.members.iter().zip(&mut clients) {
        client.request_drain().unwrap();
        let frame = client.poll_write(100).unwrap().unwrap();
        assert!(matches!(
            decode_message(&frame.bytes).unwrap(),
            RoomMessage::DrainReady { .. }
        ));
        relay
            .receive_at(member.id, &decode_message(&frame.bytes).unwrap(), 100)
            .unwrap();
        client.written(frame.id).unwrap();
    }
    for (member, client) in room.members.iter().zip(&mut clients) {
        let frame = relay.poll_write_at(member.id, 101).unwrap().unwrap();
        client
            .receive(&decode_message(&frame.bytes).unwrap(), 101)
            .unwrap();
        relay.written(member.id, frame.id).unwrap();
    }
    assert!(relay.drained());
    assert!(clients.iter().all(RoomProgressClient::drain_complete));
    (local, receipts(&clients[0]))
}

struct Script {
    snapshot: NativeRoomSnapshot,
    replies: VecDeque<NativeRoomReply>,
    commands: Vec<(u64, NativeRoomCommand)>,
    next_id: u64,
    admission_error: Option<io::ErrorKind>,
    reply_results: VecDeque<Result<(), NativeRoomFailure>>,
    hold_replies: bool,
    now: i64,
    clock_stride: i64,
    clock_values: VecDeque<i64>,
    clock_reads: usize,
    polls: usize,
    requested_stop: usize,
    joined: usize,
    after_final: Option<NativeRoomReceipts>,
    after_drain_receipts: Option<NativeRoomReceipts>,
    after_drain: Option<NativeRoomOutcome>,
    stop_override: Option<NativeRoomOutcome>,
}
#[derive(Clone)]
struct Port(Rc<RefCell<Script>>);
impl NativeRoomPort for Port {
    fn try_command(&mut self, command: NativeRoomCommand) -> io::Result<u64> {
        let mut script = self.0.borrow_mut();
        if let Some(kind) = script.admission_error.take() {
            return Err(io::Error::new(kind, "local queue refusal"));
        }
        let id = script.next_id;
        script.next_id += 1;
        script.commands.push((id, command));
        let result = script.reply_results.pop_front().unwrap_or(Ok(()));
        script.replies.push_back(NativeRoomReply { id, result });
        Ok(id)
    }
    fn poll(&self) -> io::Result<NativeRoomPoll> {
        let mut script = self.0.borrow_mut();
        script.polls += 1;
        if script.commands.iter().any(|(_, command)| {
            matches!(
                command,
                NativeRoomCommand::Publish {
                    final_prefix: true,
                    ..
                }
            )
        }) {
            if let Some(receipts) = script.after_final.take() {
                script.snapshot.receipts = receipts;
            }
        }
        if script
            .commands
            .iter()
            .any(|(_, command)| matches!(command, NativeRoomCommand::Drain))
        {
            if let Some(receipts) = script.after_drain_receipts.take() {
                script.snapshot.receipts = receipts;
            }
            if let Some(outcome) = script.after_drain.take() {
                script.snapshot.receipts = outcome.receipts;
                script.snapshot.terminal = Some(outcome);
            }
        }
        let replies = if script.hold_replies {
            Vec::new()
        } else {
            script.replies.drain(..).collect()
        };
        Ok(NativeRoomPoll {
            snapshot: script.snapshot.clone(),
            replies,
        })
    }
    fn clock_now_ns(&self) -> io::Result<i64> {
        let mut script = self.0.borrow_mut();
        script.clock_reads += 1;
        if let Some(now) = script.clock_values.pop_front() {
            script.now = now;
        }
        let now = script.now;
        let stride = script.clock_stride;
        script.now += stride;
        Ok(now)
    }
    fn request_stop(&self) {
        self.0.borrow_mut().requested_stop += 1;
    }
    fn stop(&mut self) -> NativeRoomOutcome {
        let mut script = self.0.borrow_mut();
        script.joined += 1;
        let outcome = script
            .stop_override
            .clone()
            .or_else(|| script.snapshot.terminal.clone())
            .unwrap_or(NativeRoomOutcome {
                cancelled: true,
                error: None,
                cleanup_error: None,
                receipts: script.snapshot.receipts,
                leave_written: false,
            });
        script.snapshot.receipts = outcome.receipts;
        script.snapshot.terminal = Some(outcome.clone());
        script.hold_replies = false;
        outcome
    }
}
fn port(snapshot: NativeRoomSnapshot) -> (Port, Rc<RefCell<Script>>) {
    let script = Rc::new(RefCell::new(Script {
        snapshot,
        replies: VecDeque::new(),
        commands: Vec::new(),
        next_id: 1,
        admission_error: None,
        reply_results: VecDeque::new(),
        hold_replies: false,
        now: 0,
        clock_stride: 0,
        clock_values: VecDeque::new(),
        clock_reads: 0,
        polls: 0,
        requested_stop: 0,
        joined: 0,
        after_final: None,
        after_drain_receipts: None,
        after_drain: None,
        stop_override: None,
    }));
    (Port(script.clone()), script)
}
fn controller(
    snapshot: NativeRoomSnapshot,
    players: Vec<PlayerId>,
) -> (NativeRoomCompetition<Port>, Rc<RefCell<Script>>) {
    let (port, script) = port(snapshot);
    (
        NativeRoomCompetition::new(port, players, Duration::from_secs(1)).unwrap(),
        script,
    )
}
fn failure(kind: io::ErrorKind, message: &str) -> NativeRoomFailure {
    NativeRoomFailure {
        kind,
        message: message.into(),
    }
}
fn joined_outcome(receipts: NativeRoomReceipts) -> NativeRoomOutcome {
    NativeRoomOutcome {
        cancelled: false,
        error: None,
        cleanup_error: None,
        receipts,
        leave_written: false,
    }
}

#[test]
fn explicit_lobby_requests_retain_correlations_and_recoverable_refusals_without_implicit_ready() {
    for players in [
        vec![],
        vec![PlayerId(0)],
        vec![PlayerId(7), PlayerId(7)],
        (1..=65).map(PlayerId).collect(),
    ] {
        let (port, script) = port(NativeRoomSnapshot::default());
        assert!(NativeRoomCompetition::new(port, players, Duration::from_millis(10)).is_err());
        assert!(script.borrow().commands.is_empty());
    }
    let (mut owner, script) = controller(NativeRoomSnapshot::default(), vec![PlayerId(u32::MAX)]);
    owner.poll().unwrap();
    assert!(script.borrow().commands.is_empty());
    script.borrow_mut().admission_error = Some(io::ErrorKind::WouldBlock);
    assert_eq!(owner.seal().unwrap_err().kind(), io::ErrorKind::WouldBlock);
    assert!(owner.network_error().is_none());
    script.borrow_mut().reply_results.push_back(Err(failure(
        io::ErrorKind::InvalidInput,
        "only the creator may seal",
    )));
    let seal = owner.seal().unwrap();
    let ready = owner.ready().unwrap();
    assert!(ready > seal);
    owner.poll().unwrap();
    let refused = owner.take_reply().unwrap();
    assert_eq!(refused.id, seal);
    assert_eq!(
        refused.result.unwrap_err().kind,
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        owner.take_reply().unwrap(),
        NativeRoomReply {
            id: ready,
            result: Ok(())
        }
    );
    assert!(owner.take_reply().is_none());
    assert!(owner.network_error().is_none());
    assert_eq!(script.borrow().requested_stop, 0);
    script.borrow_mut().hold_replies = true;
    for _ in 0..16 {
        owner.ready().unwrap();
    }
    let count = script.borrow().commands.len();
    assert_eq!(owner.ready().unwrap_err().kind(), io::ErrorKind::WouldBlock);
    assert_eq!(script.borrow().commands.len(), count);
    script.borrow_mut().hold_replies = false;
    owner.poll().unwrap();
    for _ in 0..16 {
        assert!(owner.take_reply().unwrap().result.is_ok());
    }
    let leave = owner.leave().unwrap();
    owner.poll().unwrap();
    assert_eq!(owner.take_reply().unwrap().id, leave);
    assert!(matches!(
        script.borrow().commands.last().unwrap().1,
        NativeRoomCommand::Leave
    ));
    let result = owner.finish(&[], false);
    assert!(result.cancelled);
    assert_eq!(script.borrow().joined, 1);
}

#[test]
fn prepared_hud_qualifies_every_remote_player_and_presentation_failure_does_not_stop_network() {
    for (hosts, slots) in [(2, 1), (3, 3), (4, 4), (64, 64)] {
        let registry = prepared(hosts, slots);
        let room = registry.room("room").unwrap();
        let (mut owner, script) = controller(snapshot(&registry), room.members[0].players.clone());
        owner.poll().unwrap();
        let count = (hosts - 1) * slots;
        let roster = owner.snapshot().room.as_ref().unwrap().clone();
        owner.poll().unwrap();
        assert!(Arc::ptr_eq(
            &roster,
            owner.snapshot().room.as_ref().unwrap()
        ));
        assert_eq!(owner.hud().unwrap().entry_count(), count);
        assert_eq!(owner.hud().unwrap().page_count(), count.div_ceil(4));
        let expected = room.members[1..]
            .iter()
            .flat_map(|member| {
                member
                    .players
                    .iter()
                    .map(move |player| (member.id, *player))
            })
            .collect::<Vec<_>>();
        let mut actual = Vec::new();
        for page in 0..count.div_ceil(4) {
            owner.set_page(page).unwrap();
            let hud = owner.hud().unwrap();
            assert!(hud.page().len() <= 4);
            actual.extend(hud.page().iter().map(|row| (row.participant, row.player)));
        }
        assert_eq!(actual, expected);
        assert!(owner.set_page(count.div_ceil(4)).is_err());
        let remote = &room.members[hosts - 1];
        let prefix = Arc::new(GroupPrefix {
            sequence: u64::MAX,
            final_prefix: true,
            members: rows(&remote.players, u64::MAX),
        });
        script
            .borrow_mut()
            .snapshot
            .peers
            .push((remote.id, prefix.clone()));
        owner.poll().unwrap();
        owner.set_page(count.div_ceil(4) - 1).unwrap();
        let last = owner.hud().unwrap().page().last().unwrap();
        assert_eq!((last.participant, last.player), *expected.last().unwrap());
        assert_eq!(last.progress, Some(prefix.members.last().unwrap().progress));
        assert!(last.final_prefix);
        assert!(last.counters()[0].contains("18446744073709551615"));
        // Inject a presentation-boundary defect. This does not assert that the
        // real common protocol would accept a second update after finality.
        let mut faulty = (*prefix).clone();
        faulty.sequence -= 1;
        script.borrow_mut().snapshot.peers[0].1 = Arc::new(faulty);
        owner.poll().unwrap();
        assert!(owner.hud_error().is_some());
        assert!(owner.hud().unwrap().failed());
        assert!(owner.network_error().is_none());
        owner.disable_hud();
        assert!(owner.hud().is_none_or(|hud| hud.failed()));
        owner.poll().unwrap();
        assert!(owner.network_error().is_none());
        let ready = owner.ready().unwrap();
        owner.poll().unwrap();
        assert_eq!(owner.take_reply().unwrap().id, ready);
        assert_eq!(script.borrow().requested_stop, 0);
        owner.finish(&[], false);
    }
    let registry = prepared(2, 2);
    let state = snapshot(&registry);
    let players = state.room.as_ref().unwrap().members[0].players.clone();
    let (mut wrong, script) = controller(state.clone(), players.iter().rev().copied().collect());
    assert!(wrong.poll().is_err());
    assert!(wrong.network_error().is_some());
    assert!(script.borrow().commands.is_empty());
    wrong.finish(&[], false);
    let (mut changed, script) = controller(state, players);
    changed.poll().unwrap();
    let mut altered = (**script.borrow().snapshot.room.as_ref().unwrap()).clone();
    altered.members[1].players.reverse();
    script.borrow_mut().snapshot.room = Some(Arc::new(altered));
    assert!(changed.poll().is_err());
    assert!(changed.network_error().is_some());
    changed.finish(&[], false);
}

#[test]
fn native_start_services_acquisition_and_preserves_real_schedule_and_host_bracket() {
    for hosts in [2, 3, 4] {
        let registry = prepared(hosts, 2);
        let schedule = schedules(&registry)[0];
        assert_eq!(schedule.song_target_ns, 11_000 + (hosts as i64 - 1) * 100);
        assert_eq!(schedule.target_ns, schedule.song_target_ns);
        assert_eq!(schedule.uncertainty_ns, 0);
        let state = snapshot(&registry);
        let players = state.room.as_ref().unwrap().members[0].players.clone();
        let (mut owner, script) = controller(state, players);
        assert!(owner.committed_schedule().is_err());
        let mut services = 0;
        assert!(
            owner
                .await_commit(&mut || {
                    services += 1;
                    if services == 2 {
                        script.borrow_mut().snapshot.schedule = Some(schedule);
                    }
                    Ok(true)
                })
                .unwrap()
        );
        assert_eq!(services, 2);
        assert_eq!(owner.committed_schedule().unwrap(), schedule);
        assert!(
            script.borrow().commands.is_empty(),
            "awaiting output cannot invent Seal or Ready"
        );
        script.borrow_mut().clock_values.extend([100, 104]);
        let mut sampled = 0;
        let bracket = owner
            .host_bracket(&mut || {
                sampled += 1;
                Ok(ClockPoint {
                    domain: ClockDomainId(77),
                    timestamp: Timestamp::from_nanos(604_800_000_000_000),
                })
            })
            .unwrap();
        assert_eq!(sampled, 1);
        let window = bracket.deadline_at(200, 104, 100).unwrap();
        assert_eq!(window.earliest().domain, ClockDomainId(77));
        assert_eq!(window.earliest().timestamp.as_nanos(), 604_800_000_000_096);
        assert_eq!(window.latest().timestamp.as_nanos(), 604_800_000_000_100);
        owner.finish(&[], false);
    }
    let registry = prepared(2, 1);
    let (mut cancelled, script) = controller(snapshot(&registry), vec![PlayerId(u32::MAX)]);
    assert!(!cancelled.await_commit(&mut || Ok(false)).unwrap());
    assert!(script.borrow().requested_stop > 0);
    assert!(script.borrow().commands.is_empty());
    let result = cancelled.finish(&[], false);
    assert!(result.cancelled);
    assert_eq!(script.borrow().joined, 1);

    let mut ended = snapshot(&registry);
    ended.schedule = Some(schedules(&registry)[0]);
    let mut terminal = joined_outcome(NativeRoomReceipts::default());
    terminal.error = Some(failure(
        io::ErrorKind::ConnectionReset,
        "failed before output",
    ));
    ended.terminal = Some(terminal);
    let (mut owner, script) = controller(ended, vec![PlayerId(u32::MAX)]);
    assert!(
        owner.await_commit(&mut || Ok(true)).is_err(),
        "historical schedule cannot override failed acquisition"
    );
    assert!(script.borrow().commands.is_empty());
    owner.finish(&[], false);

    let mut live = snapshot(&registry);
    live.schedule = Some(schedules(&registry)[0]);
    let (mut leaving, script) = controller(live, vec![PlayerId(u32::MAX)]);
    let actual = rows(&[PlayerId(u32::MAX)], 1);
    leaving.observe(&actual).unwrap();
    leaving.poll().unwrap();
    script.borrow_mut().hold_replies = true;
    let leave = leaving.leave().unwrap();
    assert!(leaving.committed_schedule().is_err());
    assert!(
        leaving
            .await_commit(&mut || panic!("pending Leave cannot service a new output start"))
            .is_err()
    );
    leaving.finish(&actual, true);
    assert_eq!(
        leaving.take_reply().unwrap().id,
        leave,
        "join retains the original correlated lobby settlement"
    );
    assert!(
        !script.borrow().commands.iter().any(|(_, command)| matches!(
            command,
            NativeRoomCommand::Drain
                | NativeRoomCommand::Publish {
                    final_prefix: true,
                    ..
                }
        ))
    );
    assert_eq!(script.borrow().joined, 1);
}

#[test]
fn ordered_local_observations_survive_cadence_backpressure_and_postcommit_network_failure() {
    let registry = prepared(3, 2);
    let state = snapshot(&registry);
    let schedule = schedules(&registry)[0];
    let players = state.room.as_ref().unwrap().members[0].players.clone();
    let (mut owner, script) = controller(state, players.clone());
    let first = rows(&players, 1);
    owner.observe(&first).unwrap();
    assert_eq!(owner.local_progress(), Some(first.as_slice()));
    assert!(script.borrow().commands.is_empty());
    script.borrow_mut().snapshot.schedule = Some(schedule);
    owner.observe(&first).unwrap();
    assert_eq!(
        script.borrow().commands,
        [(
            1,
            NativeRoomCommand::Publish {
                members: first,
                final_prefix: false
            }
        )]
    );
    script.borrow_mut().now = 49_999_999;
    owner.observe(&rows(&players, 2)).unwrap();
    assert_eq!(script.borrow().commands.len(), 1);
    script.borrow_mut().now = 50_000_000;
    script.borrow_mut().admission_error = Some(io::ErrorKind::WouldBlock);
    let third = rows(&players, 3);
    owner.observe(&third).unwrap();
    assert_eq!(owner.local_progress(), Some(third.as_slice()));
    assert!(owner.network_error().is_none());
    assert_eq!(script.borrow().commands.len(), 1);
    let fourth = rows(&players, 4);
    owner.observe(&fourth).unwrap();
    assert_eq!(
        script.borrow().commands.len(),
        2,
        "a refused queue admission cannot consume cadence credit"
    );
    assert_eq!(
        script.borrow().commands[1].1,
        NativeRoomCommand::Publish {
            members: fourth.clone(),
            final_prefix: false
        }
    );
    script.borrow_mut().hold_replies = true;
    script.borrow_mut().now = 100_000_000;
    let fifth = rows(&players, 5);
    owner.observe(&fifth).unwrap();
    assert_eq!(script.borrow().commands.len(), 2);
    let mut malformed = fifth.clone();
    malformed[1].player = malformed[0].player;
    assert!(owner.observe(&malformed).is_err());
    assert_eq!(owner.local_progress(), Some(fifth.as_slice()));
    let mut regressed = fifth.clone();
    regressed[1].progress.hits = 4;
    regressed[1].progress.combo = 4;
    regressed[1].progress.max_combo = 4;
    assert!(owner.observe(&regressed).is_err());
    assert_eq!(owner.local_progress(), Some(fifth.as_slice()));
    script.borrow_mut().hold_replies = false;
    owner.poll().unwrap();
    let mut terminal = joined_outcome(NativeRoomReceipts::default());
    terminal.error = Some(failure(
        io::ErrorKind::ConnectionReset,
        "remote closed after activation",
    ));
    script.borrow_mut().snapshot.terminal = Some(terminal);
    let sixth = rows(&players, 6);
    owner.observe(&sixth).unwrap();
    assert_eq!(owner.local_progress(), Some(sixth.as_slice()));
    assert!(owner.network_error().is_some());
    assert_eq!(
        script.borrow().commands.len(),
        2,
        "local reports continue without publishing into a failed owner"
    );
    let result = owner.finish(&sixth, false);
    assert_eq!(result.error.unwrap().kind, io::ErrorKind::ConnectionReset);
    assert_eq!(script.borrow().joined, 1);
}

#[test]
fn natural_finish_uses_real_final_and_drain_evidence_then_joins_once() {
    for hosts in [2, 3, 4] {
        let registry = prepared(hosts, 2);
        let (local, drained) = completion_evidence(&registry);
        assert!(
            local.local_final_written && local.local_final_acknowledged && local.progress_complete
        );
        assert!(!local.drain_complete);
        assert!(drained.drain_complete);
        let mut state = snapshot(&registry);
        state.schedule = Some(schedules(&registry)[0]);
        let players = state.room.as_ref().unwrap().members[0].players.clone();
        let (mut owner, script) = controller(state.clone(), players.clone());
        let actual = rows(&players, 7);
        owner.observe(&actual).unwrap();
        owner.poll().unwrap();
        script.borrow_mut().after_final = Some(local);
        script.borrow_mut().after_drain = Some(joined_outcome(drained));
        let outcome = owner.finish(&actual, true);
        assert_eq!(outcome, joined_outcome(drained));
        assert_eq!(owner.outcome(), Some(&outcome));
        {
            let script = script.borrow();
            let commands = &script.commands;
            assert_eq!(
                commands
                    .iter()
                    .filter(|(_, value)| matches!(
                        value,
                        NativeRoomCommand::Publish {
                            final_prefix: true,
                            ..
                        }
                    ))
                    .count(),
                1
            );
            assert_eq!(
                commands[commands.len() - 2].1,
                NativeRoomCommand::Publish {
                    members: actual.clone(),
                    final_prefix: true
                }
            );
            assert_eq!(commands.last().unwrap().1, NativeRoomCommand::Drain);
            assert!(
                !commands
                    .iter()
                    .any(|(_, value)| matches!(value, NativeRoomCommand::Leave))
            );
        }
        assert_eq!(script.borrow().joined, 1);
        assert_eq!(owner.finish(&[], false), outcome);
        assert_eq!(script.borrow().joined, 1);
        if hosts == 2 {
            let (mut cleanup_only, script) = controller(state, players.clone());
            cleanup_only.observe(&actual).unwrap();
            cleanup_only.poll().unwrap();
            let mut terminal = joined_outcome(drained);
            terminal.cleanup_error = Some(failure(
                io::ErrorKind::BrokenPipe,
                "completed protocol, failed stream cleanup",
            ));
            script.borrow_mut().after_drain = Some(terminal.clone());
            assert_eq!(cleanup_only.finish(&actual, true), terminal);
            assert!(cleanup_only.outcome().unwrap().error.is_none());
            assert!(cleanup_only.outcome().unwrap().cleanup_error.is_some());
            assert_eq!(script.borrow().joined, 1);
        }
    }
}

#[test]
fn cancelled_unplayed_invalid_final_and_fixed_timeout_never_fabricate_successful_drain() {
    let registry = prepared(2, 2);
    let players = registry.room("room").unwrap().members[0].players.clone();
    let mut state = snapshot(&registry);
    state.schedule = Some(schedules(&registry)[0]);
    for observed in [false, true] {
        let (mut owner, script) = controller(state.clone(), players.clone());
        let actual = rows(&players, 1);
        if observed {
            owner.observe(&actual).unwrap();
            owner.poll().unwrap();
        }
        let result = owner.finish(&actual, !observed);
        assert!(result.cancelled || result.error.is_some());
        assert!(!result.receipts.drain_complete);
        assert!(
            !script.borrow().commands.iter().any(|(_, command)| matches!(
                command,
                NativeRoomCommand::Drain
                    | NativeRoomCommand::Publish {
                        final_prefix: true,
                        ..
                    }
            ))
        );
        assert_eq!(script.borrow().joined, 1);
    }
    let (mut invalid, script) = controller(state.clone(), players.clone());
    let accepted = rows(&players, 2);
    invalid.observe(&accepted).unwrap();
    invalid.poll().unwrap();
    let mut bad = accepted.clone();
    bad.swap(0, 1);
    let result = invalid.finish(&bad, true);
    assert!(result.error.is_some());
    assert_eq!(invalid.local_progress(), Some(accepted.as_slice()));
    assert_eq!(script.borrow().commands.len(), 1);
    assert_eq!(script.borrow().joined, 1);

    let (mut refused, script) = controller(state.clone(), players.clone());
    refused.observe(&accepted).unwrap();
    refused.poll().unwrap();
    script.borrow_mut().reply_results.push_back(Err(failure(
        io::ErrorKind::InvalidInput,
        "final rejected by common owner",
    )));
    let result = refused.finish(&accepted, true);
    assert!(result.error.is_some());
    assert!(
        !script
            .borrow()
            .commands
            .iter()
            .any(|(_, command)| matches!(command, NativeRoomCommand::Drain))
    );
    assert_eq!(script.borrow().joined, 1);

    let (local, drained) = completion_evidence(&registry);
    let (mut timed, script) = controller(state, players.clone());
    let actual = rows(&players, 7);
    timed.observe(&actual).unwrap();
    timed.poll().unwrap();
    script.borrow_mut().after_final = Some(local);
    script.borrow_mut().after_drain_receipts = Some(drained);
    script
        .borrow_mut()
        .clock_values
        .extend([0, 0, 0, 0, 1_000_000_000]);
    let mut cleanup = joined_outcome(drained);
    cleanup.cancelled = true;
    cleanup.cleanup_error = Some(failure(
        io::ErrorKind::BrokenPipe,
        "stream finish failed independently",
    ));
    script.borrow_mut().stop_override = Some(cleanup);
    let result = timed.finish(&actual, true);
    assert_eq!(result.error.as_ref().unwrap().kind, io::ErrorKind::TimedOut);
    assert_eq!(
        result.cleanup_error.as_ref().unwrap().kind,
        io::ErrorKind::BrokenPipe
    );
    assert!(
        result.receipts.local_final_acknowledged
            && result.receipts.progress_complete
            && result.receipts.drain_complete
    );
    assert!(result.cancelled);
    assert_eq!(script.borrow().joined, 1);
    assert_eq!(
        script
            .borrow()
            .commands
            .iter()
            .filter(|(_, command)| matches!(command, NativeRoomCommand::Drain))
            .count(),
        1
    );
    assert!(
        script.borrow().after_drain_receipts.is_none(),
        "genuine Complete arrived before the original deadline, without joined terminal ownership"
    );
    assert!(
        !script
            .borrow()
            .commands
            .iter()
            .any(|(_, command)| matches!(command, NativeRoomCommand::Leave)),
        "real receipt flags cannot substitute an actual joined terminal or fabricate Leave"
    );
}
