//! Deferred common progress-client fixtures; no stream or application runs here.
use crate::{
    local_players::PlayerId,
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_group_rooms::{
        GroupRoomPhase, GroupRoomPolicy, GroupRoomRegistry, GroupRoomSnapshot,
    },
    multiplayer_protocol::{OutboundFrame, Progress},
    multiplayer_room_progress_client::RoomProgressClient,
    multiplayer_room_wire::{RoomMessage, decode_message, encode_message},
    multiplayer_rooms::ParticipantId,
};

fn prepared(count: usize, slots: usize) -> GroupRoomRegistry {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, count, 8, 100).unwrap());
    let players = (0..slots)
        .map(|slot| PlayerId(u32::MAX - slot as u32))
        .collect::<Vec<_>>();
    let ids = (0..count)
        .map(|_| {
            registry
                .join("room", b"client\0identity", &players, 0)
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

fn rows(players: &[PlayerId], hits: u64) -> Vec<MemberProgress> {
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

fn peer(
    participant: ParticipantId,
    players: &[PlayerId],
    sequence: u64,
    hits: u64,
    final_prefix: bool,
) -> RoomMessage {
    RoomMessage::PeerProgress {
        participant,
        prefix: GroupPrefix {
            sequence,
            final_prefix,
            members: rows(players, hits),
        },
    }
}

fn wire(frame: &OutboundFrame) -> RoomMessage {
    let message = decode_message(&frame.bytes).unwrap();
    assert_eq!(encode_message(&message).unwrap(), frame.bytes);
    message
}

#[test]
fn actual_prepared_rosters_stage_peer_data_without_publication_or_ack_before_activation() {
    for count in [2usize, 3, 4, 64] {
        let registry = prepared(count, 64);
        let snapshot = registry.room("room").unwrap();
        let own = snapshot.members[0].id;
        let remote = &snapshot.members[count - 1];
        let mut client = RoomProgressClient::new(snapshot, own).unwrap();
        assert!(!client.active());
        let before = client.clone();
        assert!(
            client
                .publish(&rows(&snapshot.members[0].players, 1), false)
                .is_err()
        );
        assert_eq!(client, before);
        let mut message = peer(remote.id, &remote.players, u64::MAX, 1, true);
        let RoomMessage::PeerProgress { prefix, .. } = &mut message else {
            unreachable!()
        };
        for member in &mut prefix.members {
            member.progress = Progress {
                song_ns: i64::MAX,
                hits: u64::MAX,
                misses: 0,
                combo: u64::MAX,
                max_combo: u64::MAX,
            };
        }
        let expected = prefix.clone();
        client.receive(&message, 10).unwrap();
        assert_eq!(client.peer_progress(remote.id), Some(&expected));
        assert!(client.peer_progress(own).is_none());
        assert!(client.poll_write(100).unwrap().is_none());
        assert!(!client.peer_final_ack_written(remote.id));
        assert!(!client.local_complete());
        client.activate().unwrap();
        assert!(client.active());
        let before = client.clone();
        assert!(client.activate().is_err());
        assert_eq!(client, before);
        let ack = client.poll_write(101).unwrap().unwrap();
        assert_eq!(
            wire(&ack),
            RoomMessage::FinalAck {
                participant: remote.id,
                sequence: u64::MAX
            }
        );
        assert!(client.poll_write(102).unwrap().is_none());
        assert!(!client.peer_final_ack_written(remote.id));
        client.written(ack.id).unwrap();
        assert!(client.peer_final_ack_written(remote.id));
        assert!(!client.local_complete());
    }
}

#[test]
fn local_coalescing_assigns_upload_sequences_only_on_admission_and_retains_final_after_partial_write()
 {
    let registry = prepared(2, 2);
    let snapshot = registry.room("room").unwrap();
    let own = &snapshot.members[0];
    let remote = &snapshot.members[1];
    let mut client = RoomProgressClient::new(snapshot, own.id).unwrap();
    client.activate().unwrap();
    client.publish(&rows(&own.players, 1), false).unwrap();
    let mut latest = rows(&own.players, 2);
    client.publish(&latest, false).unwrap();
    latest[0].progress.hits = 0;
    let first = client.poll_write(100).unwrap().unwrap();
    assert_eq!(first.id, 1);
    assert_eq!(
        wire(&first),
        RoomMessage::Progress(GroupPrefix {
            sequence: 1,
            final_prefix: false,
            members: rows(&own.players, 2)
        })
    );
    client.publish(&rows(&own.players, 3), false).unwrap();
    let before = client.clone();
    assert!(
        client.publish(&rows(&own.players, 2), false).is_err(),
        "validate against even the newest unsent publication"
    );
    assert_eq!(client, before);
    client.publish(&rows(&own.players, 4), true).unwrap();
    for final_prefix in [false, true] {
        let before = client.clone();
        assert!(
            client
                .publish(&rows(&own.players, 5), final_prefix)
                .is_err()
        );
        assert_eq!(client, before);
    }
    client
        .receive(&peer(remote.id, &remote.players, 7, 7, true), 10)
        .unwrap();
    assert!(client.poll_write(101).unwrap().is_none());
    assert!(!client.local_final_written());
    let before = client.clone();
    assert!(client.written(first.id + 1).is_err());
    assert_eq!(client, before);
    client.written(first.id).unwrap();
    let mut saw_final = false;
    let mut saw_ack = false;
    for expected_id in [2u64, 3] {
        let now = 100 + expected_id as i64 * 10;
        let frame = client.poll_write(now).unwrap().unwrap();
        assert_eq!(frame.id, expected_id);
        match wire(&frame) {
            RoomMessage::Progress(prefix) => {
                assert!(!saw_final);
                saw_final = true;
                assert_eq!(
                    prefix,
                    GroupPrefix {
                        sequence: 2,
                        final_prefix: true,
                        members: rows(&own.players, 4)
                    }
                );
                assert!(!client.local_final_written());
            }
            RoomMessage::FinalAck {
                participant,
                sequence,
            } => {
                assert!(!saw_ack);
                saw_ack = true;
                assert_eq!((participant, sequence), (remote.id, 7));
                assert!(!client.peer_final_ack_written(remote.id));
            }
            other => panic!("unexpected progress-client frame: {other:?}"),
        }
        assert!(client.poll_write(now + 1).unwrap().is_none());
        client.written(frame.id).unwrap();
    }
    assert!(saw_final && saw_ack && client.local_final_written());
    assert!(client.peer_final_ack_written(remote.id));
    assert!(!client.local_final_acknowledged());
    assert!(client.poll_write(140).unwrap().is_none());
}

#[test]
fn peer_prefixes_allow_coalesced_sequence_gaps_but_refuse_bad_later_members_and_final_replacement()
{
    let registry = prepared(3, 2);
    let snapshot = registry.room("room").unwrap();
    let own = snapshot.members[0].id;
    let remote = &snapshot.members[1];
    let mut client = RoomProgressClient::new(snapshot, own).unwrap();
    client.activate().unwrap();
    assert!(client.poll_write(1000).unwrap().is_none());
    client
        .receive(&peer(remote.id, &remote.players, 1, 1, false), 10)
        .unwrap();
    let valid = GroupPrefix {
        sequence: 9,
        final_prefix: false,
        members: rows(&remote.players, 2),
    };
    let mut bad = Vec::new();
    for sequence in [0, 1] {
        let mut prefix = valid.clone();
        prefix.sequence = sequence;
        bad.push(prefix);
    }
    let mut reordered = valid.clone();
    reordered.members.reverse();
    bad.push(reordered);
    let mut missing = valid.clone();
    missing.members.pop();
    bad.push(missing);
    let mut duplicate = valid.clone();
    duplicate.members[1].player = duplicate.members[0].player;
    bad.push(duplicate);
    let mut last = valid.clone();
    last.members[1].progress.combo = 3;
    bad.push(last);
    let mut regressed = valid.clone();
    regressed.members[1].progress.song_ns = 0;
    bad.push(regressed);
    for prefix in bad {
        let before = client.clone();
        assert!(
            client
                .receive(
                    &RoomMessage::PeerProgress {
                        participant: remote.id,
                        prefix
                    },
                    11
                )
                .is_err()
        );
        assert_eq!(client, before);
    }
    for message in [
        peer(own, &remote.players, 9, 2, false),
        peer(ParticipantId(u64::MAX), &remote.players, 9, 2, false),
        RoomMessage::Progress(valid.clone()),
        RoomMessage::Ready,
    ] {
        let before = client.clone();
        assert!(client.receive(&message, 11).is_err());
        assert_eq!(client, before);
    }
    for captured in [-1, 9] {
        let before = client.clone();
        assert!(
            client
                .receive(
                    &RoomMessage::PeerProgress {
                        participant: remote.id,
                        prefix: valid.clone()
                    },
                    captured
                )
                .is_err()
        );
        assert_eq!(client, before);
    }
    client
        .receive(
            &RoomMessage::PeerProgress {
                participant: remote.id,
                prefix: valid.clone(),
            },
            11,
        )
        .unwrap();
    assert_eq!(client.peer_progress(remote.id), Some(&valid));
    client
        .receive(&peer(remote.id, &remote.players, u64::MAX, 3, true), 12)
        .unwrap();
    for next in [
        peer(remote.id, &remote.players, u64::MAX, 3, true),
        peer(remote.id, &remote.players, 10, 4, false),
    ] {
        let before = client.clone();
        assert!(client.receive(&next, 13).is_err());
        assert_eq!(client, before);
    }
    assert!(client.peer_progress(snapshot.members[2].id).is_none());
}

#[test]
fn aggregate_ack_matches_own_final_and_original_capture_and_waits_for_actual_full_upload_write() {
    let registry = prepared(2, 1);
    let snapshot = registry.room("room").unwrap();
    let own = &snapshot.members[0];
    let other = snapshot.members[1].id;
    let mut client = RoomProgressClient::new(snapshot, own.id).unwrap();
    client.activate().unwrap();
    let ack = RoomMessage::FinalAck {
        participant: own.id,
        sequence: 1,
    };
    let before = client.clone();
    assert!(client.receive(&ack, 100).is_err());
    assert_eq!(client, before);
    client.publish(&rows(&own.players, 1), true).unwrap();
    let before = client.clone();
    assert!(client.receive(&ack, 100).is_err());
    assert_eq!(client, before);
    let final_write = client.poll_write(100).unwrap().unwrap();
    let mut after_write = client.clone();
    for (message, captured) in [
        (ack.clone(), 99),
        (
            RoomMessage::FinalAck {
                participant: own.id,
                sequence: 2,
            },
            100,
        ),
        (
            RoomMessage::FinalAck {
                participant: other,
                sequence: 1,
            },
            100,
        ),
        (
            RoomMessage::FinalAck {
                participant: ParticipantId(u64::MAX),
                sequence: 1,
            },
            100,
        ),
    ] {
        let before = client.clone();
        assert!(client.receive(&message, captured).is_err());
        assert_eq!(client, before);
    }
    client.receive(&ack, 100).unwrap();
    assert!(!client.local_final_written());
    assert!(!client.local_final_acknowledged());
    let before = client.clone();
    assert!(client.receive(&ack, 101).is_err());
    assert_eq!(client, before);
    assert!(client.written(final_write.id + 1).is_err());
    assert_eq!(client, before);
    client.written(final_write.id).unwrap();
    assert!(client.local_final_written() && client.local_final_acknowledged());
    assert!(!client.local_complete());
    after_write.written(final_write.id).unwrap();
    let before = after_write.clone();
    assert!(after_write.receive(&ack, 99).is_err());
    assert_eq!(after_write, before);
    after_write.receive(&ack, 100).unwrap();
    assert!(after_write.local_final_acknowledged());
}

#[test]
fn local_completion_requires_every_peer_final_and_every_recipient_ack_full_write() {
    for count in [2usize, 3, 4, 64] {
        let registry = prepared(count, 1);
        let snapshot = registry.room("room").unwrap();
        let own = &snapshot.members[0];
        let mut client = RoomProgressClient::new(snapshot, own.id).unwrap();
        client.activate().unwrap();
        client.publish(&rows(&own.players, 1), true).unwrap();
        let upload = client.poll_write(100).unwrap().unwrap();
        client.written(upload.id).unwrap();
        client
            .receive(
                &RoomMessage::FinalAck {
                    participant: own.id,
                    sequence: 1,
                },
                100,
            )
            .unwrap();
        assert!(!client.local_complete());
        for member in &snapshot.members[1..] {
            client
                .receive(&peer(member.id, &member.players, 7, 7, true), 101)
                .unwrap();
        }
        for completed in 1..count {
            let ack = client.poll_write(102).unwrap().unwrap();
            let RoomMessage::FinalAck {
                participant,
                sequence,
            } = wire(&ack)
            else {
                panic!("recipient ACK required")
            };
            assert_ne!(participant, own.id);
            assert_eq!(sequence, 7);
            assert!(!client.peer_final_ack_written(participant));
            assert!(!client.local_complete());
            assert!(client.poll_write(102).unwrap().is_none());
            client.written(ack.id).unwrap();
            assert!(client.peer_final_ack_written(participant));
            assert_eq!(client.local_complete(), completed == count - 1);
        }
        assert!(
            client.poll_write(103).unwrap().is_none(),
            "local receipt completion does not emit Leave"
        );
        client.stop();
        assert!(!client.active() && !client.local_complete());
        assert!(client.local_final_written() && client.local_final_acknowledged());
    }
}

#[test]
fn invalid_membership_clock_and_stop_fence_preserve_accepted_progress_without_late_receipts() {
    let registry = prepared(2, 1);
    let snapshot = registry.room("room").unwrap();
    let own = snapshot.members[0].id;
    for invalid in [
        GroupRoomSnapshot {
            phase: GroupRoomPhase::Collecting,
            ..snapshot
        },
        GroupRoomSnapshot {
            deadline_ns: Some(0),
            ..snapshot
        },
        GroupRoomSnapshot {
            members: &snapshot.members[..1],
            ..snapshot
        },
        GroupRoomSnapshot {
            identity: b"",
            ..snapshot
        },
    ] {
        assert!(RoomProgressClient::new(invalid, own).is_err());
    }
    assert!(RoomProgressClient::new(snapshot, ParticipantId(0)).is_err());
    assert!(RoomProgressClient::new(snapshot, ParticipantId(u64::MAX)).is_err());
    let mut members = snapshot.members.to_vec();
    members[0].id = ParticipantId(u64::MAX);
    members[1].id = ParticipantId(1u64 << 63);
    let mut client = RoomProgressClient::new(
        GroupRoomSnapshot {
            members: &members,
            ..snapshot
        },
        members[0].id,
    )
    .unwrap();
    client.activate().unwrap();
    client
        .receive(&peer(members[1].id, &members[1].players, 1, 1, true), 5)
        .unwrap();
    let ack = client.poll_write(100).unwrap().unwrap();
    assert_eq!(
        wire(&ack),
        RoomMessage::FinalAck {
            participant: members[1].id,
            sequence: 1
        }
    );
    for now in [-1, 99] {
        let before = client.clone();
        assert!(client.poll_write(now).is_err());
        assert_eq!(client, before);
    }
    client.stop();
    client.stop();
    let stopped = client.clone();
    assert!(client.activate().is_err());
    assert!(
        client
            .publish(&rows(&members[0].players, 1), false)
            .is_err()
    );
    assert!(
        client
            .receive(&peer(members[1].id, &members[1].players, 2, 2, true), 6)
            .is_err()
    );
    assert!(client.poll_write(101).is_err());
    assert!(client.written(ack.id).is_err());
    assert_eq!(client, stopped);
    assert!(client.peer_progress(members[1].id).unwrap().final_prefix);
    assert!(!client.peer_final_ack_written(members[1].id));
}
