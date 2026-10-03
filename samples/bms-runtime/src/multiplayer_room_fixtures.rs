//! Actual portable registry ownership with explicit times; execution is deferred.
use crate::multiplayer_rooms::{
    JoinOutcome, ParticipantId, ParticipantTicket, RoomError, RoomPolicy, RoomRegistry,
    RoomSnapshot,
};
use std::num::NonZeroU64;

fn registry(rooms: usize, key_bytes: usize, ttl: i64) -> RoomRegistry {
    RoomRegistry::new(RoomPolicy::new(rooms, key_bytes, ttl).unwrap())
}

fn waiting(owner: &mut RoomRegistry, key: &str, now: i64) -> (ParticipantTicket, i64) {
    match owner.join(key, now).unwrap() {
        JoinOutcome::Waiting {
            ticket,
            deadline_ns,
        } => (ticket, deadline_ns),
        outcome => panic!("expected a waiting ticket, got {outcome:?}"),
    }
}

fn paired(owner: &mut RoomRegistry, key: &str, now: i64) -> (ParticipantTicket, ParticipantTicket) {
    match owner.join(key, now).unwrap() {
        JoinOutcome::Paired { waiting, joined } => (waiting, joined),
        outcome => panic!("expected a pair, got {outcome:?}"),
    }
}

fn snapshot(owner: &RoomRegistry, keys: &[&str]) -> (usize, usize, Vec<Option<RoomSnapshot>>) {
    (
        owner.room_count(),
        owner.participant_count(),
        keys.iter().map(|key| owner.room(key)).collect(),
    )
}

fn ticket_ids(mut tickets: Vec<ParticipantTicket>) -> Vec<u64> {
    tickets.sort_by_key(|ticket| ticket.id.0);
    tickets.into_iter().map(|ticket| ticket.id.0).collect()
}

#[test]
fn policies_and_exact_ascii_keys_are_bounded_without_admitting_aliases() {
    for (rooms, key_bytes, ttl) in [
        (0, 1, 1),
        (4097, 1, 1),
        (1, 0, 1),
        (1, 1025, 1),
        (1, 1, 0),
        (1, 1, -1),
    ] {
        assert!(matches!(
            RoomPolicy::new(rooms, key_bytes, ttl),
            Err(RoomError::InvalidPolicy)
        ));
    }
    assert!(RoomPolicy::new(4096, 1024, i64::MAX).is_ok());
    let mut owner = registry(3, 4, 10);
    for key in [
        "", " abc", "abc ", "a/b", "a\\b", "a.b", "a%2F", "a\0b", "방", "ABCDE",
    ] {
        assert_eq!(owner.join(key, 90), Err(RoomError::InvalidKey));
        assert_eq!(snapshot(&owner, &[key]), (0, 0, vec![None]));
    }
    // Rejected keys did not advance the successful-operation clock or consume IDs.
    let (upper, _) = waiting(&mut owner, "Room", 0);
    let (lower, _) = waiting(&mut owner, "room", 0);
    let (symbols, _) = waiting(&mut owner, "_-09", 0);
    assert_eq!((upper.id.0, lower.id.0, symbols.id.0), (1, 2, 3));
    assert_eq!(
        (&upper.room[..], &lower.room[..], &symbols.room[..]),
        ("Room", "room", "_-09")
    );
    assert_ne!(owner.room("Room"), owner.room("room"));
    assert_eq!(snapshot(&owner, &[]), (3, 3, vec![]));

    let mut maximum = registry(1, 1024, i64::MAX);
    let key = "A".repeat(1024);
    let (ticket, deadline) = waiting(&mut maximum, &key, 0);
    assert_eq!(ticket.room, key);
    assert_eq!(deadline, i64::MAX);
    assert_eq!(maximum.expire(i64::MAX).unwrap(), vec![ticket]);
}

#[test]
fn full_capacity_never_evicts_and_existing_waiters_can_pair_without_new_room_capacity() {
    let mut owner = registry(2, 16, 100);
    let (a, _) = waiting(&mut owner, "A", 0);
    let (b, _) = waiting(&mut owner, "B", 1);
    let original = snapshot(&owner, &["A", "B", "C"]);
    assert_eq!(owner.join("C", 90), Err(RoomError::Capacity));
    assert_eq!(snapshot(&owner, &["A", "B", "C"]), original);
    let (first, second) = paired(&mut owner, "A", 2);
    assert_eq!(first, a);
    assert_eq!(second.id, ParticipantId(3));
    let full = snapshot(&owner, &["A", "B"]);
    assert_eq!(owner.join("A", 99), Err(RoomError::RoomFull));
    assert_eq!(snapshot(&owner, &["A", "B"]), full);
    assert_eq!(owner.release(b.id, 2).unwrap(), vec![b]);
    let (replacement, _) = waiting(&mut owner, "C", 3);
    assert_eq!(
        replacement.id,
        ParticipantId(4),
        "refusals did not consume leases"
    );
    assert_eq!(
        owner.room("A"),
        Some(RoomSnapshot::Paired {
            first: first.id,
            second: second.id
        })
    );
    assert_eq!((owner.room_count(), owner.participant_count()), (2, 3));
}

#[test]
fn expired_join_requires_explicit_ticket_return_at_the_exact_deadline() {
    let mut owner = registry(2, 16, 5);
    let (old, deadline) = waiting(&mut owner, "waiting", 10);
    assert_eq!(deadline, 15);
    let original = snapshot(&owner, &["waiting"]);
    assert_eq!(
        owner.join("waiting", 15),
        Err(RoomError::ExpiryRequired {
            id: old.id,
            deadline_ns: 15
        })
    );
    assert_eq!(snapshot(&owner, &["waiting"]), original);
    assert!(
        owner.expire(14).unwrap().is_empty(),
        "rejected future admission did not move the clock"
    );
    assert_eq!(
        owner.join("waiting", 20),
        Err(RoomError::ExpiryRequired {
            id: old.id,
            deadline_ns: 15
        })
    );
    assert_eq!(owner.expire(15).unwrap(), vec![old.clone()]);
    assert_eq!(snapshot(&owner, &["waiting"]), (0, 0, vec![None]));
    let (new, next_deadline) = waiting(&mut owner, "waiting", 15);
    assert_eq!(new.id, ParticipantId(2));
    assert_eq!(next_deadline, 20);
    assert!(owner.release(old.id, 15).unwrap().is_empty());
    assert_eq!(
        owner.room("waiting"),
        Some(RoomSnapshot::Waiting {
            id: new.id,
            deadline_ns: 20
        })
    );
    assert_eq!(owner.release(new.id, 15).unwrap(), vec![new]);
}

#[test]
fn either_pair_member_releases_both_and_stale_leases_cannot_close_recreated_rooms() {
    for release_second in [false, true] {
        let mut owner = registry(1, 16, 10);
        let (first, _) = waiting(&mut owner, "same-key", 0);
        let (matched, second) = paired(&mut owner, "same-key", 9);
        assert_eq!(matched, first);
        assert!(
            owner.expire(10_000).unwrap().is_empty(),
            "waiting TTL is not an active pair lifetime"
        );
        assert_eq!((owner.room_count(), owner.participant_count()), (1, 2));
        let released = owner
            .release(if release_second { second.id } else { first.id }, 10_000)
            .unwrap();
        assert!(released.iter().all(|ticket| ticket.room == "same-key"));
        assert_eq!(ticket_ids(released), vec![first.id.0, second.id.0]);
        assert_eq!((owner.room_count(), owner.participant_count()), (0, 0));
        let (third, _) = waiting(&mut owner, "same-key", 10_000);
        let (_, fourth) = paired(&mut owner, "same-key", 10_001);
        assert_eq!((third.id.0, fourth.id.0), (3, 4));
        for stale in [
            first.id,
            second.id,
            ParticipantId(0),
            ParticipantId(u64::MAX),
        ] {
            assert!(owner.release(stale, 10_001).unwrap().is_empty());
            assert_eq!(
                owner.room("same-key"),
                Some(RoomSnapshot::Paired {
                    first: third.id,
                    second: fourth.id
                })
            );
        }
        assert_eq!(
            ticket_ids(owner.release(fourth.id, 10_001).unwrap()),
            vec![3, 4]
        );
    }
}

#[test]
fn mixed_expiry_returns_only_due_waiting_ownership_and_preserves_active_pairs() {
    let mut owner = registry(4, 16, 10);
    let (early, _) = waiting(&mut owner, "early", 0);
    let (first, _) = waiting(&mut owner, "active", 1);
    let (_, second) = paired(&mut owner, "active", 2);
    let (middle, _) = waiting(&mut owner, "middle", 3);
    let (later, _) = waiting(&mut owner, "later", 4);
    assert_eq!((owner.room_count(), owner.participant_count()), (4, 5));
    assert_eq!(owner.expire(10).unwrap(), vec![early]);
    assert_eq!((owner.room_count(), owner.participant_count()), (3, 4));
    assert!(owner.expire(12).unwrap().is_empty());
    let removed = owner.expire(14).unwrap();
    assert!(removed.iter().any(|ticket| ticket == &middle));
    assert!(removed.iter().any(|ticket| ticket == &later));
    assert_eq!(ticket_ids(removed), vec![middle.id.0, later.id.0]);
    assert_eq!(
        owner.room("active"),
        Some(RoomSnapshot::Paired {
            first: first.id,
            second: second.id
        })
    );
    assert!(owner.expire(i64::MAX).unwrap().is_empty());
    assert_eq!(
        ticket_ids(owner.release(first.id, i64::MAX).unwrap()),
        vec![first.id.0, second.id.0]
    );
    assert_eq!((owner.room_count(), owner.participant_count()), (0, 0));
}

#[test]
fn failed_time_operations_preserve_membership_ids_and_successful_clock_baseline() {
    let mut owner = registry(2, 16, 20);
    assert_eq!(owner.join("A", -1), Err(RoomError::InvalidTime));
    assert_eq!(owner.expire(-1), Err(RoomError::InvalidTime));
    assert_eq!(
        owner.release(ParticipantId(1), -1),
        Err(RoomError::InvalidTime)
    );
    let (first, _) = waiting(&mut owner, "A", 100);
    let original = snapshot(&owner, &["A", "B"]);
    let regression = RoomError::ClockRegressed {
        previous: 100,
        now: 99,
    };
    assert_eq!(owner.join("B", 99), Err(regression));
    assert_eq!(owner.release(first.id, 99), Err(regression));
    assert_eq!(owner.expire(99), Err(regression));
    assert_eq!(snapshot(&owner, &["A", "B"]), original);
    let (second, _) = waiting(&mut owner, "B", 100);
    assert_eq!(second.id, ParticipantId(2));
    assert!(owner.release(ParticipantId(999), 101).unwrap().is_empty());
    assert_eq!(
        owner.expire(100),
        Err(RoomError::ClockRegressed {
            previous: 101,
            now: 100
        })
    );
    assert_eq!(owner.join("bad/key", 500), Err(RoomError::InvalidKey));
    assert_eq!(owner.release(first.id, 101).unwrap(), vec![first]);
    let (third, _) = waiting(&mut owner, "C", 101);
    assert_eq!(third.id, ParticipantId(3));
    assert_eq!((owner.room_count(), owner.participant_count()), (2, 2));
}

#[test]
fn id_and_deadline_overflow_are_atomic_and_pairing_does_not_compute_an_unused_deadline() {
    let policy = RoomPolicy::new(2, 16, 10).unwrap();
    let mut owner = RoomRegistry::with_next_id(policy, NonZeroU64::new(u64::MAX - 1).unwrap());
    let (first, _) = waiting(&mut owner, "A", 0);
    let (_, last) = paired(&mut owner, "A", 1);
    assert_eq!((first.id.0, last.id.0), (u64::MAX - 1, u64::MAX));
    let full = snapshot(&owner, &["A", "B"]);
    assert_eq!(owner.join("B", 9), Err(RoomError::IdExhausted));
    assert_eq!(snapshot(&owner, &["A", "B"]), full);
    assert_eq!(
        ticket_ids(owner.release(last.id, 1).unwrap()),
        vec![u64::MAX - 1, u64::MAX]
    );
    assert_eq!(owner.join("A", 1), Err(RoomError::IdExhausted));
    assert_eq!((owner.room_count(), owner.participant_count()), (0, 0));

    let mut deadline = registry(2, 16, 10);
    assert_eq!(
        deadline.join("overflow", i64::MAX - 9),
        Err(RoomError::DeadlineOverflow)
    );
    assert_eq!(snapshot(&deadline, &["overflow"]), (0, 0, vec![None]));
    let (first, due) = waiting(&mut deadline, "edge", i64::MAX - 10);
    assert_eq!(
        first.id,
        ParticipantId(1),
        "deadline failure did not consume the first lease"
    );
    assert_eq!(due, i64::MAX);
    let (matched, second) = paired(&mut deadline, "edge", i64::MAX - 1);
    assert_eq!(matched, first);
    assert_eq!(second.id, ParticipantId(2));
    assert!(deadline.expire(i64::MAX).unwrap().is_empty());
    assert_eq!(
        ticket_ids(deadline.release(second.id, i64::MAX).unwrap()),
        vec![1, 2]
    );
}
