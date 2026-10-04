//! Deferred registry ownership fixtures; tickets do not assert stream closure.
use super::*;

fn registry(rooms: usize, hosts: usize, key_bytes: usize, ttl: i64) -> GroupRoomRegistry {
    GroupRoomRegistry::new(GroupRoomPolicy::new(rooms, hosts, key_bytes, ttl).unwrap())
}

type OwnedSnapshot = (
    Vec<u8>,
    Vec<(ParticipantId, Vec<PlayerId>, bool)>,
    GroupRoomPhase,
    Option<i64>,
);

fn snapshot(owner: &GroupRoomRegistry, key: &str) -> Option<OwnedSnapshot> {
    owner.room(key).map(|room| {
        (
            room.identity.to_vec(),
            room.members
                .iter()
                .map(|member| (member.id, member.players.clone(), member.prepared))
                .collect(),
            room.phase,
            room.deadline_ns,
        )
    })
}

#[test]
fn policy_keys_identity_and_whole_local_rosters_are_bounded_owned_and_atomic() {
    for args in [
        (0, 2, 1, 1),
        (4097, 2, 1, 1),
        (1, 1, 1, 1),
        (1, 65, 1, 1),
        (1, 2, 0, 1),
        (1, 2, 1025, 1),
        (1, 2, 1, 0),
        (1, 2, 1, -1),
    ] {
        assert_eq!(
            GroupRoomPolicy::new(args.0, args.1, args.2, args.3),
            Err(GroupRoomError::InvalidPolicy)
        );
    }
    assert!(GroupRoomPolicy::new(4096, 64, 1024, i64::MAX).is_ok());
    let mut owner = registry(3, 3, 4, 100);
    for key in [
        "", " abc", "abc ", "a/b", "a\\b", "a.b", "a%2F", "a\0b", "방", "ABCDE",
    ] {
        assert_eq!(
            owner.join(key, b"setup", &[PlayerId(1)], 90),
            Err(GroupRoomError::InvalidKey)
        );
    }
    for identity in [vec![], vec![1; 65_537]] {
        assert_eq!(
            owner.join("Room", &identity, &[PlayerId(1)], 90),
            Err(GroupRoomError::InvalidIdentity)
        );
    }
    for players in [
        vec![],
        vec![PlayerId(0)],
        vec![PlayerId(1), PlayerId(1)],
        (1..=65).map(PlayerId).collect(),
    ] {
        assert_eq!(
            owner.join("Room", b"setup", &players, 90),
            Err(GroupRoomError::InvalidRoster)
        );
    }
    assert_eq!((owner.room_count(), owner.participant_count()), (0, 0));
    let mut identity = vec![0, 255, 17];
    let mut players = vec![PlayerId(u32::MAX), PlayerId(7)];
    let first = owner.join("Room", &identity, &players, 0).unwrap();
    assert_eq!(first.id, ParticipantId(1));
    identity[0] = 9;
    players[1] = PlayerId(8);
    let original = snapshot(&owner, "Room");
    assert_eq!(owner.room("Room").unwrap().identity, &[0, 255, 17]);
    assert_eq!(
        owner.room("Room").unwrap().members[0].players,
        [PlayerId(u32::MAX), PlayerId(7)]
    );
    assert_eq!(
        owner.join("Room", &identity, &players, 90),
        Err(GroupRoomError::IdentityMismatch)
    );
    assert_eq!(snapshot(&owner, "Room"), original);
    let second = owner
        .join("Room", &[0, 255, 17], &[PlayerId(7)], 1)
        .unwrap();
    let lower = owner.join("room", b"other", &[PlayerId(7)], 1).unwrap();
    let symbols = owner.join("_-09", b"other", &[PlayerId(7)], 1).unwrap();
    assert_eq!((second.id.0, lower.id.0, symbols.id.0), (2, 3, 4));
    assert_eq!((owner.room_count(), owner.participant_count()), (3, 4));
    assert_eq!(lower.room, "room");
    assert_eq!(first.room, "Room");

    let mut maximum = registry(1, 2, 1024, i64::MAX);
    let key = "A".repeat(1024);
    let identity = vec![0xA5; 65_536];
    let ticket = maximum
        .join(&key, &identity, &[PlayerId(u32::MAX)], 0)
        .unwrap();
    assert_eq!(maximum.room(&key).unwrap().deadline_ns, Some(i64::MAX));
    assert_eq!(maximum.room(&key).unwrap().identity, identity.as_slice());
    assert_eq!(maximum.expire(i64::MAX).unwrap(), vec![ticket]);
}

#[test]
fn two_three_four_and_sixty_four_hosts_freeze_scoped_rosters_until_every_host_is_prepared() {
    let players = (0..64)
        .map(|index| PlayerId(u32::MAX - index))
        .collect::<Vec<_>>();
    for hosts in [2usize, 3, 4, 64] {
        let mut owner = registry(1, hosts, 16, 10);
        let tickets = (0..hosts)
            .map(|_| owner.join("cohort", b"canonical", &players, 0).unwrap())
            .collect::<Vec<_>>();
        assert_eq!((owner.room_count(), owner.participant_count()), (1, hosts));
        let room = owner.room("cohort").unwrap();
        assert_eq!(room.phase, GroupRoomPhase::Collecting);
        assert_eq!(room.deadline_ns, Some(10));
        assert_eq!(room.members.len(), hosts);
        for (index, member) in room.members.iter().enumerate() {
            assert_eq!(member.id, tickets[index].id);
            assert_eq!(member.players, players);
            assert!(!member.prepared);
        }
        assert_ne!(tickets[0].id, tickets[1].id);
        owner.seal(tickets[0].id, 1).unwrap();
        assert_eq!(owner.room("cohort").unwrap().phase, GroupRoomPhase::Frozen);
        for (index, ticket) in tickets.iter().enumerate() {
            let all_prepared = owner.ready(ticket.id, 2).unwrap();
            assert_eq!(all_prepared, index + 1 == hosts);
            let room = owner.room("cohort").unwrap();
            assert_eq!(
                room.phase,
                if all_prepared {
                    GroupRoomPhase::Prepared
                } else {
                    GroupRoomPhase::Frozen
                }
            );
            assert_eq!(room.deadline_ns, if all_prepared { None } else { Some(10) });
            assert_eq!(
                room.members.iter().filter(|member| member.prepared).count(),
                index + 1
            );
        }
        // Prepared describes readiness only; the registry supplies no start or ACK.
        assert!(owner.expire(i64::MAX).unwrap().is_empty());
        assert_eq!(
            owner.release(tickets[hosts - 1].id, i64::MAX).unwrap(),
            tickets
        );
        assert_eq!((owner.room_count(), owner.participant_count()), (0, 0));
    }
}

#[test]
fn seal_authority_phase_and_capacity_refusals_leave_clock_ids_and_prepared_bits_unchanged() {
    let mut owner = registry(1, 3, 16, 100);
    let a = owner.join("room", b"setup", &[PlayerId(1)], 0).unwrap();
    let first = snapshot(&owner, "room");
    assert_eq!(owner.seal(a.id, 90), Err(GroupRoomError::TooFewHosts));
    assert_eq!(owner.ready(a.id, 90), Err(GroupRoomError::NotFrozen));
    assert_eq!(
        owner.seal(ParticipantId(999), 90),
        Err(GroupRoomError::UnknownParticipant)
    );
    assert_eq!(snapshot(&owner, "room"), first);
    let b = owner.join("room", b"setup", &[PlayerId(1)], 1).unwrap();
    let c = owner.join("room", b"setup", &[PlayerId(1)], 2).unwrap();
    assert_eq!((a.id.0, b.id.0, c.id.0), (1, 2, 3));
    let full = snapshot(&owner, "room");
    assert_eq!(
        owner.join("room", b"setup", &[PlayerId(1)], 90),
        Err(GroupRoomError::RoomFull)
    );
    assert_eq!(
        owner.join("other", b"setup", &[PlayerId(1)], 90),
        Err(GroupRoomError::Capacity)
    );
    assert_eq!(owner.seal(b.id, 90), Err(GroupRoomError::NotOwner));
    assert_eq!(snapshot(&owner, "room"), full);
    owner.seal(a.id, 3).unwrap();
    let frozen = snapshot(&owner, "room");
    assert_eq!(owner.seal(a.id, 90), Err(GroupRoomError::RoomFrozen));
    assert_eq!(
        owner.join("room", b"setup", &[PlayerId(1)], 90),
        Err(GroupRoomError::RoomFrozen)
    );
    assert_eq!(snapshot(&owner, "room"), frozen);
    assert!(!owner.ready(b.id, 4).unwrap());
    let partial = snapshot(&owner, "room");
    assert_eq!(owner.ready(b.id, 90), Err(GroupRoomError::AlreadyPrepared));
    assert_eq!(
        owner.ready(ParticipantId(999), 90),
        Err(GroupRoomError::UnknownParticipant)
    );
    assert_eq!(snapshot(&owner, "room"), partial);
    assert!(!owner.ready(a.id, 4).unwrap());
    assert!(owner.ready(c.id, 5).unwrap());
    assert_eq!(owner.release(c.id, 5).unwrap(), vec![a, b, c]);
    assert_eq!(
        owner.join("other", b"setup", &[PlayerId(1)], 5).unwrap().id,
        ParticipantId(4)
    );
}

#[test]
fn releasing_any_live_lease_returns_the_whole_room_and_stale_ids_cannot_close_replacements() {
    for phase in [
        GroupRoomPhase::Collecting,
        GroupRoomPhase::Frozen,
        GroupRoomPhase::Prepared,
    ] {
        let mut owner = registry(2, 4, 16, 10);
        let old = (0..4)
            .map(|_| {
                owner
                    .join("same", b"setup", &[PlayerId(u32::MAX)], 0)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let unrelated = owner
            .join("other", b"different", &[PlayerId(1)], 0)
            .unwrap();
        if phase != GroupRoomPhase::Collecting {
            owner.seal(old[0].id, 1).unwrap();
        }
        if phase == GroupRoomPhase::Prepared {
            for ticket in &old {
                owner.ready(ticket.id, 1).unwrap();
            }
        }
        assert_eq!(owner.room("same").unwrap().phase, phase);
        assert_eq!(owner.release(old[2].id, 2).unwrap(), old);
        assert!(owner.room("same").is_none());
        assert_eq!((owner.room_count(), owner.participant_count()), (1, 1));
        assert_eq!(owner.room("other").unwrap().members[0].id, unrelated.id);
        let replacement = owner
            .join("same", b"new-identity", &[PlayerId(7)], 2)
            .unwrap();
        assert_eq!(replacement.id, ParticipantId(6));
        let current = snapshot(&owner, "same");
        for stale in old
            .iter()
            .map(|ticket| ticket.id)
            .chain([ParticipantId(0), ParticipantId(u64::MAX)])
        {
            assert!(owner.release(stale, 2).unwrap().is_empty());
            assert_eq!(snapshot(&owner, "same"), current);
        }
        assert_eq!(owner.release(replacement.id, 2).unwrap(), vec![replacement]);
        assert_eq!(owner.release(unrelated.id, 2).unwrap(), vec![unrelated]);
    }
}

#[test]
fn original_deadline_survives_sealing_and_partial_readiness_without_implicit_eviction() {
    let mut owner = registry(3, 3, 16, 10);
    let a = owner.join("A", b"setup", &[PlayerId(1)], 0).unwrap();
    let b = owner.join("B", b"setup", &[PlayerId(1)], 1).unwrap();
    let b2 = owner.join("B", b"setup", &[PlayerId(1)], 1).unwrap();
    owner.seal(b.id, 2).unwrap();
    assert!(!owner.ready(b.id, 2).unwrap());
    assert!(owner.ready(b2.id, 2).unwrap());
    let waiting = owner.join("waiting", b"setup", &[PlayerId(1)], 3).unwrap();
    let a2 = owner.join("A", b"setup", &[PlayerId(1)], 9).unwrap();
    owner.seal(a.id, 9).unwrap();
    assert!(!owner.ready(a.id, 9).unwrap());
    assert_eq!(owner.room("A").unwrap().deadline_ns, Some(10));
    let before = snapshot(&owner, "A");
    let expired = GroupRoomError::ExpiryRequired {
        owner: a.id,
        deadline_ns: 10,
    };
    assert_eq!(owner.join("A", b"setup", &[PlayerId(1)], 20), Err(expired));
    assert_eq!(owner.seal(a.id, 20), Err(expired));
    assert_eq!(owner.ready(a2.id, 20), Err(expired));
    assert_eq!(snapshot(&owner, "A"), before);
    assert!(owner.expire(9).unwrap().is_empty());
    assert_eq!(owner.expire(10).unwrap(), vec![a.clone(), a2]);
    assert!(owner.room("A").is_none());
    assert_eq!(owner.expire(13).unwrap(), vec![waiting]);
    assert_eq!(owner.room("B").unwrap().phase, GroupRoomPhase::Prepared);
    let replacement = owner.join("A", b"setup", &[PlayerId(1)], 13).unwrap();
    assert_eq!(replacement.id, ParticipantId(6));
    assert!(owner.release(a.id, 13).unwrap().is_empty());
    assert_eq!(owner.expire(23).unwrap(), vec![replacement]);
    assert!(owner.expire(i64::MAX).unwrap().is_empty());
    assert_eq!(owner.release(b.id, i64::MAX).unwrap(), vec![b, b2]);

    let mut edge = registry(1, 2, 16, 10);
    assert_eq!(
        edge.join("edge", b"setup", &[PlayerId(1)], i64::MAX - 9),
        Err(GroupRoomError::DeadlineOverflow)
    );
    let first = edge
        .join("edge", b"setup", &[PlayerId(1)], i64::MAX - 10)
        .unwrap();
    let second = edge
        .join("edge", b"setup", &[PlayerId(1)], i64::MAX - 1)
        .unwrap();
    assert_eq!((first.id.0, second.id.0), (1, 2));
    edge.seal(first.id, i64::MAX - 1).unwrap();
    assert!(!edge.ready(first.id, i64::MAX - 1).unwrap());
    assert!(edge.ready(second.id, i64::MAX - 1).unwrap());
    assert!(edge.expire(i64::MAX).unwrap().is_empty());
}

#[test]
fn checked_clock_and_exhausted_leases_are_atomic_while_stop_irreversibly_returns_all_ownership() {
    let mut edge = registry(1, 2, 16, 100);
    edge.next_id = Some(u64::MAX - 1);
    assert_eq!(
        edge.join("edge", b"setup", &[PlayerId(1)], -1),
        Err(GroupRoomError::InvalidTime)
    );
    assert_eq!(edge.stop(-1), Err(GroupRoomError::InvalidTime));
    let first = edge.join("edge", b"setup", &[PlayerId(1)], 10).unwrap();
    let regression = GroupRoomError::ClockRegressed {
        previous: 10,
        now: 9,
    };
    let before = snapshot(&edge, "edge");
    assert_eq!(
        edge.join("edge", b"setup", &[PlayerId(1)], 9),
        Err(regression)
    );
    assert_eq!(edge.seal(first.id, 9), Err(regression));
    assert_eq!(edge.ready(first.id, 9), Err(regression));
    assert_eq!(edge.release(first.id, 9), Err(regression));
    assert_eq!(edge.expire(9), Err(regression));
    assert_eq!(edge.stop(9), Err(regression));
    assert_eq!(snapshot(&edge, "edge"), before);
    let last = edge.join("edge", b"setup", &[PlayerId(1)], 10).unwrap();
    assert_eq!((first.id.0, last.id.0), (u64::MAX - 1, u64::MAX));
    assert_eq!(edge.release(last.id, 10).unwrap(), vec![first, last]);
    assert_eq!(
        edge.join("edge", b"setup", &[PlayerId(1)], 90),
        Err(GroupRoomError::IdExhausted)
    );
    assert!(edge.expire(10).unwrap().is_empty());
    assert_eq!((edge.room_count(), edge.participant_count()), (0, 0));

    let mut owner = registry(3, 3, 16, 100);
    let z = owner.join("Z", b"setup", &[PlayerId(1)], 0).unwrap();
    let a = owner.join("A", b"setup", &[PlayerId(1)], 0).unwrap();
    let a2 = owner.join("A", b"setup", &[PlayerId(1)], 0).unwrap();
    let m = owner.join("M", b"setup", &[PlayerId(1)], 0).unwrap();
    let m2 = owner.join("M", b"setup", &[PlayerId(1)], 0).unwrap();
    owner.seal(a.id, 1).unwrap();
    owner.ready(a.id, 1).unwrap();
    owner.ready(a2.id, 1).unwrap();
    owner.seal(m.id, 1).unwrap();
    assert_eq!(owner.stop(2).unwrap(), vec![a.clone(), a2, m, m2, z]);
    assert_eq!((owner.room_count(), owner.participant_count()), (0, 0));
    assert_eq!(
        owner.join("new", b"setup", &[PlayerId(1)], 2),
        Err(GroupRoomError::Stopped)
    );
    assert_eq!(owner.seal(a.id, 2), Err(GroupRoomError::Stopped));
    assert_eq!(owner.ready(a.id, 2), Err(GroupRoomError::Stopped));
    assert!(owner.stop(3).unwrap().is_empty());
    assert!(owner.release(a.id, 3).unwrap().is_empty());
    assert!(owner.expire(3).unwrap().is_empty());
    assert_eq!(
        owner.expire(2),
        Err(GroupRoomError::ClockRegressed {
            previous: 3,
            now: 2
        })
    );
}
