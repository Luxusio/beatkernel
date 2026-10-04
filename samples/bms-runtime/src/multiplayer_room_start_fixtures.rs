//! Deferred software-start fixtures; no network, hardware clock, or output is run.
use crate::{
    local_players::PlayerId,
    multiplayer_clock::{ClockError, ClockFilter, ClockSample, OffsetEstimate},
    multiplayer_group_rooms::{
        GroupRoomPhase, GroupRoomPolicy, GroupRoomRegistry, GroupRoomSnapshot,
    },
    multiplayer_room_start::{RoomStartCoordinator, RoomStartError},
    multiplayer_rooms::ParticipantId,
    multiplayer_start::{StartAgreement, StartError, StartMessage, StartPolicy, StartRole},
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

fn prepared_registry(count: usize) -> GroupRoomRegistry {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, count, 8, 100).unwrap());
    let players = (0..64)
        .map(|index| PlayerId(u32::MAX - index))
        .collect::<Vec<_>>();
    let ids = (0..count)
        .map(|_| {
            registry
                .join("room", b"canonical\0identity", &players, 0)
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

fn coordinator(count: usize, policy: StartPolicy) -> RoomStartCoordinator {
    let registry = prepared_registry(count);
    RoomStartCoordinator::new(registry.room("room").unwrap(), policy).unwrap()
}

fn estimate(local: i64, remote: i64, width: i64) -> OffsetEstimate {
    let mut filter = ClockFilter::new();
    filter
        .observe(
            ClockSample::new(
                local,
                remote + width / 2,
                remote + width / 2 + 10,
                local + width + 10,
            )
            .unwrap(),
        )
        .unwrap();
    filter.estimate().unwrap()
}

fn exact(observed: i64) -> OffsetEstimate {
    let mut filter = ClockFilter::new();
    filter
        .observe(ClockSample::new(observed, observed, observed, observed).unwrap())
        .unwrap();
    filter.estimate().unwrap()
}

fn ready(count: usize, now: i64, policy: StartPolicy) -> RoomStartCoordinator {
    let mut room = coordinator(count, policy);
    for id in room.participants().to_vec() {
        room.prepare(id, exact(now), now).unwrap();
        let mut join = StartAgreement::new(StartRole::Join, policy).unwrap();
        join.prepare(exact(now)).unwrap();
        let host_ready = room.next(id, now).unwrap().unwrap();
        let peer_ready = join.next(now).unwrap().unwrap();
        room.written(id, host_ready, now).unwrap();
        join.receive(host_ready, now).unwrap();
        join.written(peer_ready, now).unwrap();
        room.receive(id, peer_ready, now).unwrap();
    }
    room
}

#[test]
fn constructor_owns_only_complete_prepared_rosters_including_full_width_scoped_ids() {
    for count in [2usize, 3, 4, 64] {
        let registry = prepared_registry(count);
        let snapshot = registry.room("room").unwrap();
        let room = RoomStartCoordinator::new(snapshot, policy()).unwrap();
        assert_eq!(
            room.participants(),
            snapshot
                .members
                .iter()
                .map(|member| member.id)
                .collect::<Vec<_>>()
        );
        assert_eq!(room.song_target_ns(), None);
        assert!(!room.committed());
        let mut members = snapshot.members.to_vec();
        for (index, member) in members.iter_mut().enumerate() {
            member.id = ParticipantId(u64::MAX - index as u64);
        }
        let actual_ids = members.iter().map(|member| member.id).collect::<Vec<_>>();
        let mut full_width = RoomStartCoordinator::new(
            GroupRoomSnapshot {
                members: &members,
                ..snapshot
            },
            policy(),
        )
        .unwrap();
        members[0].players.clear();
        members[0].id = ParticipantId(0);
        assert_eq!(full_width.participants(), actual_ids);
        for id in &actual_ids {
            full_width.prepare(*id, exact(0), 0).unwrap();
            let message = full_width.next(*id, 0).unwrap().unwrap();
            full_width.written(*id, message, 0).unwrap();
            full_width
                .receive(*id, StartMessage::ClockReady(0), 0)
                .unwrap();
        }
        assert_eq!(
            full_width.next(actual_ids[0], 0).unwrap(),
            Some(StartMessage::Propose(10_000))
        );
    }
    let registry = prepared_registry(2);
    let snapshot = registry.room("room").unwrap();
    for identity in [b"".as_slice(), &[7; 65_537]] {
        assert_eq!(
            RoomStartCoordinator::new(
                GroupRoomSnapshot {
                    identity,
                    ..snapshot
                },
                policy()
            )
            .unwrap_err(),
            RoomStartError::InvalidRoom
        );
    }
    for phase in [GroupRoomPhase::Collecting, GroupRoomPhase::Frozen] {
        assert!(
            RoomStartCoordinator::new(GroupRoomSnapshot { phase, ..snapshot }, policy()).is_err()
        );
    }
    assert!(
        RoomStartCoordinator::new(
            GroupRoomSnapshot {
                deadline_ns: Some(0),
                ..snapshot
            },
            policy()
        )
        .is_err()
    );
    for count in [0usize, 1, 65] {
        let members = (0..count)
            .map(|index| {
                let mut member = snapshot.members[0].clone();
                member.id = ParticipantId(index as u64 + 1);
                member
            })
            .collect::<Vec<_>>();
        assert!(
            RoomStartCoordinator::new(
                GroupRoomSnapshot {
                    members: &members,
                    ..snapshot
                },
                policy()
            )
            .is_err()
        );
    }
    for fault in 0..6 {
        let mut members = snapshot.members.to_vec();
        match fault {
            0 => members[1].id = ParticipantId(0),
            1 => members[1].id = members[0].id,
            2 => members[1].prepared = false,
            3 => members[1].players.clear(),
            4 => members[1].players = vec![PlayerId(0)],
            _ => members[1].players = vec![PlayerId(7), PlayerId(7)],
        }
        assert!(
            RoomStartCoordinator::new(
                GroupRoomSnapshot {
                    members: &members,
                    ..snapshot
                },
                policy()
            )
            .is_err()
        );
    }
    for invalid in [
        StartPolicy {
            lead_ns: 0,
            ..policy()
        },
        StartPolicy {
            min_remaining_ns: 0,
            ..policy()
        },
        StartPolicy {
            max_age_ns: u64::MAX,
            ..policy()
        },
    ] {
        assert_eq!(
            RoomStartCoordinator::new(snapshot, invalid).unwrap_err(),
            RoomStartError::Start(StartError::InvalidPolicy)
        );
    }
}

#[test]
fn actual_join_agreements_share_one_target_only_after_every_ready_accept_and_commit_write() {
    for count in [2usize, 3, 4, 64] {
        let mut room = coordinator(count, policy());
        let ids = room.participants().to_vec();
        let base = 1_000_000i64;
        let now = base + 1_000;
        let mut joins = Vec::new();
        let mut offsets = Vec::new();
        let mut prerolls = Vec::new();
        let mut widths = Vec::new();
        let mut held_ready = None;
        for (index, id) in ids.iter().copied().enumerate() {
            let offset = [-200, 50, 700, -30][index % 4];
            let preroll = [0, 300, 1_700, 600][index % 4];
            let width = [20, 40, 60, 80][index % 4];
            room.prepare(id, estimate(base, base + offset, width), now)
                .unwrap();
            let mut join = StartAgreement::new_at(StartRole::Join, policy(), preroll).unwrap();
            join.prepare(estimate(base + offset, base, width)).unwrap();
            let host_ready = room.next(id, now).unwrap().unwrap();
            assert_eq!(host_ready, StartMessage::ClockReady(0));
            assert_eq!(room.next(id, now).unwrap(), None);
            let peer_ready = join.next(now + offset).unwrap().unwrap();
            assert_eq!(peer_ready, StartMessage::ClockReady(preroll));
            join.written(peer_ready, now + offset).unwrap();
            if index + 1 == count {
                held_ready = Some((host_ready, peer_ready));
            } else {
                room.written(id, host_ready, now).unwrap();
                join.receive(host_ready, now + offset).unwrap();
                room.receive(id, peer_ready, now).unwrap();
            }
            joins.push(join);
            offsets.push(offset);
            prerolls.push(preroll);
            widths.push(width);
        }
        assert_eq!(room.next(ids[0], now).unwrap(), None);
        assert_eq!(room.song_target_ns(), None);
        let last = count - 1;
        let (host_ready, peer_ready) = held_ready.unwrap();
        room.written(ids[last], host_ready, now).unwrap();
        joins[last]
            .receive(host_ready, now + offsets[last])
            .unwrap();
        assert_eq!(
            room.next(ids[0], now).unwrap(),
            None,
            "a full local Ready write is not the peer's Ready receipt"
        );
        room.receive(ids[last], peer_ready, now).unwrap();

        let chosen_at = base + 2_000;
        let expected =
            chosen_at + 10_000 + prerolls.iter().max().unwrap() + widths.iter().max().unwrap();
        let mut accepts = Vec::new();
        for index in 0..count {
            let time = chosen_at + index as i64;
            let proposal = room.next(ids[index], time).unwrap().unwrap();
            assert_eq!(
                proposal,
                StartMessage::Propose(expected),
                "later polling cannot select another target"
            );
            assert_eq!(room.song_target_ns(), Some(expected));
            room.written(ids[index], proposal, time).unwrap();
            joins[index]
                .receive(proposal, time + offsets[index])
                .unwrap();
            let accept = joins[index].next(time + offsets[index]).unwrap().unwrap();
            joins[index].written(accept, time + offsets[index]).unwrap();
            accepts.push(accept);
        }
        let acceptance_time = chosen_at + count as i64;
        for index in 0..last {
            room.receive(ids[index], accepts[index], acceptance_time)
                .unwrap();
        }
        for id in &ids {
            assert_eq!(room.next(*id, acceptance_time).unwrap(), None);
        }
        assert!(!room.committed());
        room.receive(ids[last], accepts[last], acceptance_time)
            .unwrap();
        let mut last_commit = None;
        for index in 0..count {
            let time = acceptance_time + index as i64;
            let commit = room.next(ids[index], time).unwrap().unwrap();
            assert_eq!(commit, StartMessage::Commit(expected));
            assert!(!room.committed());
            room.written(ids[index], commit, time).unwrap();
            if index == last {
                last_commit = Some((commit, time));
            } else {
                joins[index].receive(commit, time + offsets[index]).unwrap();
            }
        }
        assert!(room.committed());
        assert!(
            !joins[last].committed(),
            "all server writes do not claim a remote application acknowledgement"
        );
        let (commit, time) = last_commit.unwrap();
        joins[last].receive(commit, time + offsets[last]).unwrap();
        for index in 0..count {
            let schedule = joins[index].take_schedule().unwrap();
            assert_eq!(schedule.song_target_ns, expected + offsets[index]);
            assert_eq!(
                schedule.target_ns,
                expected + offsets[index] - prerolls[index]
            );
            assert_eq!(schedule.uncertainty_ns, widths[index] as u64);
            assert!(joins[index].committed());
            assert_eq!(joins[index].take_schedule(), None);
        }
    }
}

#[test]
fn rejected_preparation_clock_and_identity_calls_do_not_adopt_a_partial_peer_or_global_time() {
    let mut room = coordinator(3, policy());
    let ids = room.participants().to_vec();
    let before = room.clone();
    for result in [
        room.prepare(ParticipantId(u64::MAX), exact(0), 0),
        room.prepare(ids[0], exact(10), 9),
        room.prepare(ids[0], exact(0), 100_001),
        room.prepare(ids[0], estimate(0, 0, 102), 112),
        room.prepare(ids[0], exact(0), -1),
    ] {
        assert!(result.is_err());
    }
    assert_eq!(room, before);
    assert_eq!(
        room.next(ParticipantId(u64::MAX), 50),
        Err(RoomStartError::UnknownParticipant)
    );
    assert_eq!(
        room.written(ParticipantId(u64::MAX), StartMessage::ClockReady(0), 50),
        Err(RoomStartError::UnknownParticipant)
    );
    assert_eq!(room, before);
    assert_eq!(
        room.prepare(ids[0], exact(10), 9),
        Err(RoomStartError::Start(StartError::Estimate(
            ClockError::FutureObservation
        )))
    );
    assert_eq!(room, before);
    room.prepare(ids[0], exact(0), 0).unwrap();
    let before = room.clone();
    assert_eq!(
        room.prepare(ids[0], exact(1), 1),
        Err(RoomStartError::Start(StartError::AlreadyPrepared))
    );
    assert_eq!(room, before);
    assert!(
        room.receive(ids[1], StartMessage::ClockReady(-1), 50)
            .is_err()
    );
    assert_eq!(room, before);
    assert!(
        room.receive(ParticipantId(0), StartMessage::ClockReady(0), 50)
            .is_err()
    );
    assert_eq!(room, before);
    let ready = room.next(ids[0], 10).unwrap().unwrap();
    let before = room.clone();
    assert!(room.next(ids[1], 9).is_err());
    assert!(room.written(ids[0], ready, 9).is_err());
    assert_eq!(room, before);
    room.written(ids[0], ready, 10).unwrap();
    room.prepare(ids[1], exact(10), 10).unwrap();
    assert_eq!(room.song_target_ns(), None);
}

#[test]
fn all_peer_proposal_preflight_and_exact_echo_receipts_roll_back_without_hiding_a_later_failure() {
    let bounded_age = StartPolicy {
        max_age_ns: 50,
        ..policy()
    };
    let mut room = coordinator(2, bounded_age);
    let ids = room.participants().to_vec();
    for (id, observed) in [(ids[0], 100), (ids[1], 60)] {
        room.prepare(id, exact(observed), 100).unwrap();
        let ready = room.next(id, 100).unwrap().unwrap();
        room.written(id, ready, 100).unwrap();
        room.receive(id, StartMessage::ClockReady(0), 100).unwrap();
    }
    let before = room.clone();
    assert_eq!(
        room.next(ids[0], 111),
        Err(RoomStartError::Start(StartError::Estimate(
            ClockError::StaleEstimate
        )))
    );
    assert_eq!(room, before);
    assert_eq!(room.song_target_ns(), None);
    let proposal = room.next(ids[0], 110).unwrap().unwrap();
    let target = match proposal {
        StartMessage::Propose(target) => target,
        _ => unreachable!(),
    };
    let before = room.clone();
    assert!(
        room.receive(ids[0], StartMessage::Accept(target), 110)
            .is_err()
    );
    assert!(
        room.written(ids[0], StartMessage::Propose(target + 1), 110)
            .is_err()
    );
    assert_eq!(room, before);
    room.written(ids[0], proposal, 110).unwrap();
    let before = room.clone();
    assert_eq!(
        room.receive(ids[0], StartMessage::Accept(target + 1), 110),
        Err(RoomStartError::Start(StartError::WrongEcho))
    );
    assert!(room.written(ids[0], proposal, 110).is_err());
    assert!(
        room.receive(ids[0], StartMessage::ClockReady(0), 110)
            .is_err()
    );
    assert_eq!(room, before);
    room.receive(ids[0], StartMessage::Accept(target), 110)
        .unwrap();
    assert_eq!(room.next(ids[0], 110).unwrap(), None);
    let other = room.next(ids[1], 110).unwrap().unwrap();
    assert_eq!(other, proposal);
    room.written(ids[1], other, 110).unwrap();
    room.receive(ids[1], StartMessage::Accept(target), 110)
        .unwrap();
    let commit = room.next(ids[1], 110).unwrap().unwrap();
    let before = room.clone();
    assert!(
        room.written(ids[1], commit, 111).is_err(),
        "the exact in-flight peer still requires a fresh estimate at full write"
    );
    assert_eq!(room, before);
}

#[test]
fn long_reference_clocks_overflow_and_stop_preserve_the_actual_historical_target() {
    for now in [72_000_000_000_000i64, 604_800_000_000_000] {
        let mut room = ready(4, now, policy());
        let ids = room.participants().to_vec();
        let target = now + 10_000;
        for id in &ids {
            let proposal = room.next(*id, now).unwrap().unwrap();
            assert_eq!(proposal, StartMessage::Propose(target));
            room.written(*id, proposal, now).unwrap();
            room.receive(*id, StartMessage::Accept(target), now)
                .unwrap();
        }
        for id in &ids {
            let commit = room.next(*id, now).unwrap().unwrap();
            room.written(*id, commit, now).unwrap();
        }
        assert!(room.committed());
        room.stop();
        room.stop();
        assert!(!room.committed());
        assert_eq!(room.song_target_ns(), Some(target));
        let stopped = room.clone();
        assert_eq!(
            room.prepare(ids[0], exact(now), now),
            Err(RoomStartError::Stopped)
        );
        assert_eq!(room.next(ids[0], now), Err(RoomStartError::Stopped));
        assert_eq!(
            room.receive(ids[0], StartMessage::Accept(target), now),
            Err(RoomStartError::Stopped)
        );
        assert_eq!(
            room.written(ids[0], StartMessage::Commit(target), now),
            Err(RoomStartError::Stopped)
        );
        assert_eq!(room, stopped);
    }
    let now = i64::MAX - 10_000;
    let mut boundary = ready(2, now, policy());
    let id = boundary.participants()[0];
    assert_eq!(
        boundary.next(id, now).unwrap(),
        Some(StartMessage::Propose(i64::MAX))
    );
    let mut overflow = ready(2, now + 1, policy());
    let before = overflow.clone();
    assert_eq!(
        overflow.next(id, now + 1),
        Err(RoomStartError::Start(StartError::Overflow))
    );
    assert_eq!(overflow, before);
    assert_eq!(overflow.song_target_ns(), None);

    let mut preroll_overflow = coordinator(2, policy());
    for id in preroll_overflow.participants().to_vec() {
        preroll_overflow.prepare(id, exact(0), 0).unwrap();
        let message = preroll_overflow.next(id, 0).unwrap().unwrap();
        preroll_overflow.written(id, message, 0).unwrap();
        preroll_overflow
            .receive(id, StartMessage::ClockReady(i64::MAX), 0)
            .unwrap();
    }
    let before = preroll_overflow.clone();
    assert_eq!(
        preroll_overflow.next(id, 0),
        Err(RoomStartError::Start(StartError::Overflow))
    );
    assert_eq!(preroll_overflow, before);
    preroll_overflow.stop();
    assert_eq!(preroll_overflow.song_target_ns(), None);
}

#[test]
fn existing_bilateral_host_and_join_keep_original_preroll_and_offset_schedule_semantics() {
    let mut host = StartAgreement::new_at(StartRole::Host, policy(), 300).unwrap();
    let mut join = StartAgreement::new_at(StartRole::Join, policy(), 700).unwrap();
    host.prepare(estimate(1_000, 1_050, 20)).unwrap();
    join.prepare(estimate(1_050, 1_000, 20)).unwrap();
    let host_ready = host.next(2_000).unwrap().unwrap();
    let join_ready = join.next(2_050).unwrap().unwrap();
    host.written(host_ready, 2_000).unwrap();
    join.receive(host_ready, 2_050).unwrap();
    join.written(join_ready, 2_050).unwrap();
    host.receive(join_ready, 2_000).unwrap();
    let proposal = host.next(2_000).unwrap().unwrap();
    assert_eq!(proposal, StartMessage::Propose(12_700));
    host.written(proposal, 2_001).unwrap();
    join.receive(proposal, 2_051).unwrap();
    let accept = join.next(2_052).unwrap().unwrap();
    join.written(accept, 2_053).unwrap();
    host.receive(accept, 2_003).unwrap();
    let commit = host.next(2_004).unwrap().unwrap();
    host.written(commit, 2_005).unwrap();
    join.receive(commit, 2_055).unwrap();
    let host_schedule = host.take_schedule().unwrap();
    let join_schedule = join.take_schedule().unwrap();
    assert_eq!(
        (host_schedule.song_target_ns, host_schedule.target_ns),
        (12_700, 12_400)
    );
    assert_eq!(
        (join_schedule.song_target_ns, join_schedule.target_ns),
        (12_750, 12_050)
    );
    assert_eq!(join_schedule.uncertainty_ns, 20);
}
