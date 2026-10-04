//! Deferred common relay fixtures; no transport or gameplay publication runs here.
use crate::{
    local_players::PlayerId,
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_group_rooms::{
        GroupRoomPhase, GroupRoomPolicy, GroupRoomRegistry, GroupRoomSnapshot,
    },
    multiplayer_protocol::Progress,
    multiplayer_room_progress::{RoomProgressRelay, RoomRelayWrite},
    multiplayer_room_progress_client::RoomProgressClient,
    multiplayer_room_wire::{RoomMessage, decode_message, encode_message},
    multiplayer_rooms::ParticipantId,
};
use std::sync::Arc;

fn prepared(count: usize, players: usize) -> GroupRoomRegistry {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, count, 8, 100).unwrap());
    let roster = (0..players)
        .map(|slot| PlayerId(u32::MAX - slot as u32))
        .collect::<Vec<_>>();
    let ids = (0..count)
        .map(|_| {
            registry
                .join("room", b"relay\0identity", &roster, 0)
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

fn prefix(players: &[PlayerId], sequence: u64, final_prefix: bool) -> GroupPrefix {
    GroupPrefix {
        sequence,
        final_prefix,
        members: players
            .iter()
            .map(|&player| MemberProgress {
                player,
                progress: Progress {
                    song_ns: 604_800_000_000_000 + sequence as i64,
                    hits: sequence,
                    misses: 0,
                    combo: sequence,
                    max_combo: sequence,
                },
            })
            .collect(),
    }
}

fn decoded(frame: &RoomRelayWrite) -> RoomMessage {
    let message = decode_message(&frame.bytes).unwrap();
    assert_eq!(
        encode_message(&message).unwrap().as_slice(),
        frame.bytes.as_slice()
    );
    message
}

fn pending_aggregates(
    snapshot: GroupRoomSnapshot<'_>,
    timed: bool,
) -> (RoomProgressRelay, Vec<(ParticipantId, RoomRelayWrite)>) {
    let mut relay = RoomProgressRelay::new(snapshot).unwrap();
    relay.activate().unwrap();
    for member in snapshot.members {
        relay
            .receive_at(
                member.id,
                &RoomMessage::Progress(prefix(&member.players, 1, true)),
                10,
            )
            .unwrap();
    }
    let mut acknowledgements = Vec::new();
    for member in snapshot.members {
        for _ in 1..snapshot.members.len() {
            let frame = relay.poll_write_at(member.id, 20).unwrap().unwrap();
            let RoomMessage::PeerProgress {
                participant,
                prefix,
            } = decoded(&frame)
            else {
                panic!("final peer prefix required")
            };
            relay.written(member.id, frame.id).unwrap();
            acknowledgements.push((
                member.id,
                RoomMessage::FinalAck {
                    participant,
                    sequence: prefix.sequence,
                },
            ));
        }
    }
    for (recipient, acknowledgement) in acknowledgements {
        relay.receive_at(recipient, &acknowledgement, 30).unwrap();
    }
    let aggregates = snapshot
        .members
        .iter()
        .map(|member| {
            let frame = if timed {
                relay.poll_write_at(member.id, 40)
            } else {
                relay.poll_write(member.id)
            }
            .unwrap()
            .unwrap();
            assert_eq!(
                decoded(&frame),
                RoomMessage::FinalAck {
                    participant: member.id,
                    sequence: 1
                }
            );
            (member.id, frame)
        })
        .collect();
    (relay, aggregates)
}

#[test]
fn real_prepared_two_three_four_sixty_four_host_rosters_stage_then_share_one_owned_prefix() {
    for count in [2usize, 3, 4, 64] {
        let registry = prepared(count, 64);
        let snapshot = registry.room("room").unwrap();
        let source = snapshot.members[0].id;
        let mut relay = RoomProgressRelay::new(snapshot).unwrap();
        assert!(!relay.active());
        let mut original = prefix(&snapshot.members[0].players, 1, false);
        for member in &mut original.members {
            member.progress = Progress {
                song_ns: i64::MAX,
                hits: u64::MAX,
                misses: 0,
                combo: u64::MAX,
                max_combo: u64::MAX,
            };
        }
        let expected = original.clone();
        relay
            .receive(source, &RoomMessage::Progress(original.clone()))
            .unwrap();
        original.members[0].progress.hits = 0;
        for member in snapshot.members {
            assert!(relay.poll_write(member.id).unwrap().is_none());
        }
        assert!(!relay.complete());
        relay.activate().unwrap();
        assert!(relay.active());
        let before = relay.clone();
        assert!(relay.activate().is_err());
        assert_eq!(relay, before);
        assert!(
            relay.poll_write(source).unwrap().is_none(),
            "never echo to the upload lease"
        );
        let mut shared = None;
        for recipient in &snapshot.members[1..] {
            let frame = relay.poll_write(recipient.id).unwrap().unwrap();
            assert_eq!(frame.id, 1);
            assert_eq!(frame.final_source, None);
            assert_eq!(
                decoded(&frame),
                RoomMessage::PeerProgress {
                    participant: source,
                    prefix: expected.clone()
                }
            );
            if let Some(bytes) = &shared {
                assert!(Arc::ptr_eq(bytes, &frame.bytes));
            } else {
                shared = Some(frame.bytes.clone());
            }
            assert!(relay.poll_write(recipient.id).unwrap().is_none());
            relay.written(recipient.id, frame.id).unwrap();
            assert!(relay.poll_write(recipient.id).unwrap().is_none());
        }
        assert!(!relay.final_acknowledged(source).unwrap());
    }
}

#[test]
fn rejected_uploads_preserve_exact_sequence_roster_and_every_previous_member() {
    let registry = prepared(3, 2);
    let snapshot = registry.room("room").unwrap();
    let source = snapshot.members[0].id;
    let players = &snapshot.members[0].players;
    let mut relay = RoomProgressRelay::new(snapshot).unwrap();
    relay
        .receive(source, &RoomMessage::Progress(prefix(players, 1, false)))
        .unwrap();
    let valid = prefix(players, 2, false);
    let mut invalid = Vec::new();
    for sequence in [0, 1, 3, u64::MAX] {
        let mut next = valid.clone();
        next.sequence = sequence;
        invalid.push(next);
    }
    let mut reordered = valid.clone();
    reordered.members.reverse();
    invalid.push(reordered);
    let mut missing = valid.clone();
    missing.members.pop();
    invalid.push(missing);
    let mut duplicate = valid.clone();
    duplicate.members[1].player = duplicate.members[0].player;
    invalid.push(duplicate);
    let mut zero = valid.clone();
    zero.members[1].player = PlayerId(0);
    invalid.push(zero);
    let mut bad_last = valid.clone();
    bad_last.members[1].progress.combo = 3;
    invalid.push(bad_last);
    let mut regressed = valid.clone();
    regressed.members[1].progress.song_ns = 0;
    invalid.push(regressed);
    let mut overflow = valid.clone();
    overflow.members[1].progress.hits = u64::MAX;
    overflow.members[1].progress.misses = 1;
    invalid.push(overflow);
    for next in invalid {
        let before = relay.clone();
        assert!(relay.receive(source, &RoomMessage::Progress(next)).is_err());
        assert_eq!(relay, before);
    }
    for (lease, message) in [
        (
            ParticipantId(u64::MAX),
            RoomMessage::Progress(valid.clone()),
        ),
        (
            source,
            RoomMessage::PeerProgress {
                participant: source,
                prefix: valid.clone(),
            },
        ),
        (source, RoomMessage::Seal),
    ] {
        let before = relay.clone();
        assert!(relay.receive(lease, &message).is_err());
        assert_eq!(relay, before);
    }
    relay
        .receive(source, &RoomMessage::Progress(valid))
        .unwrap();
    let final_prefix = prefix(players, 3, true);
    relay
        .receive(source, &RoomMessage::Progress(final_prefix.clone()))
        .unwrap();
    for next in [
        final_prefix.clone(),
        prefix(players, 4, false),
        prefix(players, 4, true),
    ] {
        let before = relay.clone();
        assert!(relay.receive(source, &RoomMessage::Progress(next)).is_err());
        assert_eq!(relay, before);
    }
    relay.activate().unwrap();
    let frame = relay.poll_write(snapshot.members[1].id).unwrap().unwrap();
    assert_eq!(
        decoded(&frame),
        RoomMessage::PeerProgress {
            participant: source,
            prefix: final_prefix
        }
    );
}

#[test]
fn recipient_round_robin_preserves_inflight_bytes_and_coalesces_only_unsent_prefixes() {
    let registry = prepared(4, 1);
    let snapshot = registry.room("room").unwrap();
    let mut relay = RoomProgressRelay::new(snapshot).unwrap();
    relay.activate().unwrap();
    for source in &snapshot.members[..3] {
        relay
            .receive(
                source.id,
                &RoomMessage::Progress(prefix(&source.players, 1, false)),
            )
            .unwrap();
    }
    let recipient = snapshot.members[3].id;
    let first = relay.poll_write(recipient).unwrap().unwrap();
    let RoomMessage::PeerProgress {
        participant: busy_source,
        prefix: first_prefix,
    } = decoded(&first)
    else {
        panic!("peer prefix required")
    };
    let source = snapshot
        .members
        .iter()
        .find(|member| member.id == busy_source)
        .unwrap();
    let before = relay.clone();
    assert!(
        relay
            .receive(
                recipient,
                &RoomMessage::FinalAck {
                    participant: busy_source,
                    sequence: 1
                }
            )
            .is_err()
    );
    assert_eq!(
        relay, before,
        "an ordinary in-flight prefix cannot receive a final ACK"
    );
    for sequence in 2..=20 {
        relay
            .receive(
                busy_source,
                &RoomMessage::Progress(prefix(&source.players, sequence, false)),
            )
            .unwrap();
        assert!(relay.poll_write(recipient).unwrap().is_none());
    }
    assert_eq!(
        decoded(&first),
        RoomMessage::PeerProgress {
            participant: busy_source,
            prefix: first_prefix
        }
    );
    let before = relay.clone();
    assert!(relay.written(recipient, first.id + 1).is_err());
    assert_eq!(relay, before);
    relay.written(recipient, first.id).unwrap();
    let mut seen = vec![busy_source];
    for expected_id in 2..=3 {
        let frame = relay.poll_write(recipient).unwrap().unwrap();
        assert_eq!(frame.id, expected_id);
        let RoomMessage::PeerProgress {
            participant,
            prefix,
        } = decoded(&frame)
        else {
            panic!("peer prefix required")
        };
        assert!(
            !seen.contains(&participant),
            "a busy source cannot starve another dirty source"
        );
        assert_eq!(prefix.sequence, 1);
        seen.push(participant);
        relay.written(recipient, frame.id).unwrap();
    }
    let latest = relay.poll_write(recipient).unwrap().unwrap();
    assert_eq!(latest.id, 4);
    assert_eq!(
        decoded(&latest),
        RoomMessage::PeerProgress {
            participant: busy_source,
            prefix: prefix(&source.players, 20, false)
        }
    );
    relay.written(recipient, latest.id).unwrap();
    assert!(relay.poll_write(recipient).unwrap().is_none());
}

#[test]
fn exact_early_final_ack_waits_for_its_full_write_and_never_credits_duplicates_or_other_frames() {
    let registry = prepared(2, 1);
    let snapshot = registry.room("room").unwrap();
    let source = snapshot.members[0].id;
    let recipient = snapshot.members[1].id;
    let final_prefix = prefix(&snapshot.members[0].players, 1, true);
    let ack = RoomMessage::FinalAck {
        participant: source,
        sequence: 1,
    };
    let mut relay = RoomProgressRelay::new(snapshot).unwrap();
    relay
        .receive(source, &RoomMessage::Progress(final_prefix.clone()))
        .unwrap();
    let before = relay.clone();
    assert!(
        relay.receive(recipient, &ack).is_err(),
        "inactive staging has no ACK authority"
    );
    assert_eq!(relay, before);
    relay.activate().unwrap();
    let before = relay.clone();
    assert!(
        relay.receive(recipient, &ack).is_err(),
        "no relay frame was offered yet"
    );
    assert_eq!(relay, before);
    let frame = relay.poll_write(recipient).unwrap().unwrap();
    assert_eq!(frame.final_source, Some(source));
    for (lease, bad) in [
        (source, ack.clone()),
        (ParticipantId(u64::MAX), ack.clone()),
        (
            recipient,
            RoomMessage::FinalAck {
                participant: source,
                sequence: 2,
            },
        ),
        (
            recipient,
            RoomMessage::FinalAck {
                participant: ParticipantId(u64::MAX),
                sequence: 1,
            },
        ),
        (
            recipient,
            RoomMessage::FinalAck {
                participant: recipient,
                sequence: 1,
            },
        ),
    ] {
        let before = relay.clone();
        assert!(relay.receive(lease, &bad).is_err());
        assert_eq!(relay, before);
    }
    relay.receive(recipient, &ack).unwrap();
    assert!(!relay.final_acknowledged(source).unwrap());
    assert!(relay.poll_write(source).unwrap().is_none());
    let before = relay.clone();
    assert!(relay.receive(recipient, &ack).is_err());
    assert!(relay.written(recipient, frame.id + 1).is_err());
    assert_eq!(relay, before);
    relay.written(recipient, frame.id).unwrap();
    assert!(relay.final_acknowledged(source).unwrap());
    let aggregate = relay.poll_write(source).unwrap().unwrap();
    assert_eq!(aggregate.final_source, None);
    assert_eq!(decoded(&aggregate), ack);
    assert!(!relay.complete());
    relay.written(source, aggregate.id).unwrap();
    assert!(
        !relay.complete(),
        "the other source has not uploaded its final prefix"
    );
    assert!(relay.poll_write(source).unwrap().is_none());
    let before = relay.clone();
    assert!(relay.receive(recipient, &ack).is_err());
    assert!(relay.written(source, aggregate.id).is_err());
    assert_eq!(relay, before);
}

#[test]
fn all_required_recipient_acks_and_all_aggregate_full_writes_are_needed_for_room_completion() {
    for count in [2usize, 3, 4, 64] {
        let registry = prepared(count, 1);
        let snapshot = registry.room("room").unwrap();
        let mut relay = RoomProgressRelay::new(snapshot).unwrap();
        relay.activate().unwrap();
        for source in snapshot.members {
            relay
                .receive(
                    source.id,
                    &RoomMessage::Progress(prefix(&source.players, 1, true)),
                )
                .unwrap();
        }
        let mut delivered = Vec::new();
        for recipient in snapshot.members {
            for _ in 0..count - 1 {
                let frame = relay.poll_write(recipient.id).unwrap().unwrap();
                let RoomMessage::PeerProgress {
                    participant,
                    prefix,
                } = decoded(&frame)
                else {
                    panic!("actual final relay required")
                };
                assert_ne!(participant, recipient.id);
                assert!(prefix.final_prefix);
                relay.written(recipient.id, frame.id).unwrap();
                delivered.push((recipient.id, participant, prefix.sequence));
            }
            assert!(relay.poll_write(recipient.id).unwrap().is_none());
        }
        for source in snapshot.members {
            assert!(!relay.final_acknowledged(source.id).unwrap());
        }
        let withheld_source = snapshot.members[0].id;
        let withheld_recipient = snapshot.members[count - 1].id;
        for (recipient, source, sequence) in delivered {
            if (recipient, source) != (withheld_recipient, withheld_source) {
                relay
                    .receive(
                        recipient,
                        &RoomMessage::FinalAck {
                            participant: source,
                            sequence,
                        },
                    )
                    .unwrap();
            }
        }
        assert!(!relay.final_acknowledged(withheld_source).unwrap());
        assert!(relay.poll_write(withheld_source).unwrap().is_none());
        relay
            .receive(
                withheld_recipient,
                &RoomMessage::FinalAck {
                    participant: withheld_source,
                    sequence: 1,
                },
            )
            .unwrap();
        let mut aggregates = Vec::new();
        for source in snapshot.members {
            assert!(relay.final_acknowledged(source.id).unwrap());
            let frame = relay.poll_write(source.id).unwrap().unwrap();
            assert_eq!(
                decoded(&frame),
                RoomMessage::FinalAck {
                    participant: source.id,
                    sequence: 1
                }
            );
            aggregates.push((source.id, frame.id));
        }
        assert!(!relay.complete());
        for (index, (source, id)) in aggregates.into_iter().enumerate() {
            relay.written(source, id).unwrap();
            assert_eq!(relay.complete(), index + 1 == count);
        }
        for member in snapshot.members {
            assert!(relay.poll_write(member.id).unwrap().is_none());
        }
        relay.stop();
        assert!(!relay.complete());
        assert!(!relay.active());
    }
}

#[test]
fn malformed_prepared_snapshots_full_width_host_identity_and_stop_preserve_bounded_ownership() {
    let registry = prepared(2, 1);
    let snapshot = registry.room("room").unwrap();
    for invalid in [
        GroupRoomSnapshot {
            phase: GroupRoomPhase::Collecting,
            ..snapshot
        },
        GroupRoomSnapshot {
            phase: GroupRoomPhase::Frozen,
            ..snapshot
        },
        GroupRoomSnapshot {
            deadline_ns: Some(0),
            ..snapshot
        },
        GroupRoomSnapshot {
            identity: b"",
            ..snapshot
        },
        GroupRoomSnapshot {
            members: &snapshot.members[..1],
            ..snapshot
        },
    ] {
        assert!(RoomProgressRelay::new(invalid).is_err());
    }
    for kind in 0..5 {
        let mut members = snapshot.members.to_vec();
        match kind {
            0 => members[1].id = ParticipantId(0),
            1 => members[1].id = members[0].id,
            2 => {
                let duplicate = members[1].players[0];
                members[1].players.push(duplicate);
            }
            3 => members[1].prepared = false,
            _ => members.resize(65, members[0].clone()),
        }
        assert!(
            RoomProgressRelay::new(GroupRoomSnapshot {
                members: &members,
                ..snapshot
            })
            .is_err()
        );
    }
    let mut members = snapshot.members.to_vec();
    members[0].id = ParticipantId(u64::MAX);
    members[1].id = ParticipantId(1u64 << 63);
    let mut relay = RoomProgressRelay::new(GroupRoomSnapshot {
        members: &members,
        ..snapshot
    })
    .unwrap();
    relay.activate().unwrap();
    relay
        .receive(
            members[0].id,
            &RoomMessage::Progress(prefix(&members[0].players, 1, false)),
        )
        .unwrap();
    let frame = relay.poll_write(members[1].id).unwrap().unwrap();
    let RoomMessage::PeerProgress { participant, .. } = decoded(&frame) else {
        panic!("peer prefix required")
    };
    assert_eq!(participant, ParticipantId(u64::MAX));
    relay.stop();
    relay.stop();
    assert!(!relay.active());
    assert!(!relay.complete());
    assert!(relay.activate().is_err());
    assert!(relay.poll_write(members[1].id).is_err());
    assert!(relay.written(members[1].id, frame.id).is_err());
    assert!(
        relay
            .receive(
                members[0].id,
                &RoomMessage::Progress(prefix(&members[0].players, 2, false))
            )
            .is_err()
    );
}

#[test]
fn actual_clients_and_relay_drain_two_three_four_and_sixty_four_hosts_only_after_every_receipt() {
    for count in [2usize, 3, 4, 64] {
        let registry = prepared(count, 1);
        let snapshot = registry.room("room").unwrap();
        let mut relay = RoomProgressRelay::new(snapshot).unwrap();
        relay.activate().unwrap();
        let mut clients = snapshot
            .members
            .iter()
            .map(|member| {
                let mut client = RoomProgressClient::new(snapshot, member.id).unwrap();
                client.activate().unwrap();
                client
                    .publish(&prefix(&member.players, 1, true).members, true)
                    .unwrap();
                client
            })
            .collect::<Vec<_>>();
        for (member, client) in snapshot.members.iter().zip(&mut clients) {
            let upload = client.poll_write(10).unwrap().unwrap();
            relay
                .receive_at(member.id, &decode_message(&upload.bytes).unwrap(), 20)
                .unwrap();
            client.written(upload.id).unwrap();
        }
        for (member, client) in snapshot.members.iter().zip(&mut clients) {
            for _ in 1..count {
                let frame = relay.poll_write_at(member.id, 30).unwrap().unwrap();
                client.receive(&decoded(&frame), 40).unwrap();
                relay.written(member.id, frame.id).unwrap();
            }
        }
        for (member, client) in snapshot.members.iter().zip(&mut clients) {
            for _ in 1..count {
                let ack = client.poll_write(50).unwrap().unwrap();
                relay
                    .receive_at(member.id, &decode_message(&ack.bytes).unwrap(), 60)
                    .unwrap();
                client.written(ack.id).unwrap();
            }
            assert!(!client.local_complete());
        }
        let mut aggregates = Vec::new();
        let mut ready_frames = Vec::new();
        for (member, client) in snapshot.members.iter().zip(&mut clients) {
            let aggregate = relay.poll_write_at(member.id, 70).unwrap().unwrap();
            assert_eq!(aggregate.id, count as u64);
            client.receive(&decoded(&aggregate), 80).unwrap();
            assert!(client.local_complete());
            assert!(!client.drain_complete());
            client.request_drain().unwrap();
            let ready = client.poll_write(90).unwrap().unwrap();
            assert_eq!(
                ready.id,
                count as u64 + 1,
                "drain shares the actual upload/ACK write-ID space"
            );
            assert_eq!(
                decode_message(&ready.bytes).unwrap(),
                RoomMessage::DrainReady {
                    participant: member.id,
                    sequence: 1
                }
            );
            ready_frames.push(ready);
            aggregates.push(aggregate);
        }
        for index in 0..count - 1 {
            relay
                .receive_at(
                    snapshot.members[index].id,
                    &decode_message(&ready_frames[index].bytes).unwrap(),
                    100,
                )
                .unwrap();
            clients[index].written(ready_frames[index].id).unwrap();
        }
        assert!(!relay.complete() && !relay.drained());
        for (member, aggregate) in snapshot.members.iter().zip(&aggregates) {
            relay.written(member.id, aggregate.id).unwrap();
        }
        assert!(relay.complete());
        for member in snapshot.members {
            assert!(
                relay.poll_write_at(member.id, 110).unwrap().is_none(),
                "one host has not requested drain"
            );
        }
        let last = count - 1;
        relay
            .receive_at(
                snapshot.members[last].id,
                &decode_message(&ready_frames[last].bytes).unwrap(),
                100,
            )
            .unwrap();
        clients[last].written(ready_frames[last].id).unwrap();
        for (index, (member, client)) in snapshot.members.iter().zip(&mut clients).enumerate() {
            let notice = relay.poll_write_at(member.id, 120).unwrap().unwrap();
            assert_eq!(notice.id, count as u64 + 1);
            assert_eq!(notice.final_source, None);
            assert_eq!(
                decoded(&notice),
                RoomMessage::DrainComplete {
                    participant: member.id,
                    sequence: 1
                }
            );
            assert!(!relay.drained());
            assert!(relay.poll_write_at(member.id, 121).unwrap().is_none());
            client.receive(&decoded(&notice), 130).unwrap();
            assert!(
                client.drain_complete(),
                "this client needs no unrelated recipient's notice write"
            );
            relay.written(member.id, notice.id).unwrap();
            assert_eq!(relay.drained(), index + 1 == count);
            assert!(
                client.poll_write(140).unwrap().is_none(),
                "drain does not fabricate Leave or another protocol frame"
            );
        }
        assert!(clients.iter().all(RoomProgressClient::drain_complete));
        relay.stop();
        assert!(
            !relay.drained(),
            "Stop revokes even a fully written coordinated drain"
        );
    }
}

#[test]
fn drain_ready_requires_exact_lease_sequence_capture_and_full_aggregate_receipt_atomically() {
    let registry = prepared(3, 1);
    let snapshot = registry.room("room").unwrap();
    let (mut relay, aggregates) = pending_aggregates(snapshot, true);
    let own = snapshot.members[0].id;
    let ready = RoomMessage::DrainReady {
        participant: own,
        sequence: 1,
    };
    for (lease, message, captured) in [
        (own, ready.clone(), -1),
        (own, ready.clone(), 29),
        (own, ready.clone(), 39),
        (ParticipantId(u64::MAX), ready.clone(), 40),
        (snapshot.members[1].id, ready.clone(), 40),
        (
            own,
            RoomMessage::DrainReady {
                participant: own,
                sequence: 2,
            },
            40,
        ),
        (
            own,
            RoomMessage::DrainComplete {
                participant: own,
                sequence: 1,
            },
            40,
        ),
    ] {
        let before = relay.clone();
        assert!(relay.receive_at(lease, &message, captured).is_err());
        assert_eq!(relay, before);
    }
    let before = relay.clone();
    assert!(
        relay.receive(own, &ready).is_err(),
        "untimed receive supplies no original capture"
    );
    assert_eq!(relay, before);
    relay.receive_at(own, &ready, 40).unwrap();
    let pending = relay.clone();
    assert!(relay.receive_at(own, &ready, 41).is_err());
    assert!(relay.written(own, aggregates[0].1.id + 1).is_err());
    assert_eq!(relay, pending);
    for (recipient, aggregate) in &aggregates[1..] {
        relay
            .receive_at(
                *recipient,
                &RoomMessage::DrainReady {
                    participant: *recipient,
                    sequence: 1,
                },
                40,
            )
            .unwrap();
        relay.written(*recipient, aggregate.id).unwrap();
    }
    for member in snapshot.members {
        assert!(
            relay.poll_write_at(member.id, 50).unwrap().is_none(),
            "early readiness cannot credit the remaining aggregate write"
        );
    }
    assert!(!relay.complete() && !relay.drained());
    relay.written(own, aggregates[0].1.id).unwrap();
    assert!(relay.complete());
    let complete = relay.poll_write_at(own, 51).unwrap().unwrap();
    assert_eq!(
        decoded(&complete),
        RoomMessage::DrainComplete {
            participant: own,
            sequence: 1
        }
    );
    let before = relay.clone();
    assert!(relay.receive_at(own, &ready, 51).is_err());
    assert!(relay.written(own, aggregates[0].1.id).is_err());
    assert_eq!(relay, before);
    relay.written(own, complete.id).unwrap();
    assert!(
        !relay.drained(),
        "the other actual Complete notices still need full writes"
    );
}

#[test]
fn untimed_aggregate_and_stop_never_gain_drain_authority_from_later_polling_or_receipts() {
    let registry = prepared(2, 1);
    let snapshot = registry.room("room").unwrap();
    let (mut legacy, aggregates) = pending_aggregates(snapshot, false);
    for (recipient, aggregate) in &aggregates {
        assert!(legacy.poll_write_at(*recipient, 100).unwrap().is_none());
        legacy.written(*recipient, aggregate.id).unwrap();
        let before = legacy.clone();
        assert!(
            legacy
                .receive_at(
                    *recipient,
                    &RoomMessage::DrainReady {
                        participant: *recipient,
                        sequence: 1
                    },
                    101
                )
                .is_err()
        );
        assert_eq!(
            legacy, before,
            "an untimed aggregate cannot acquire a fictional admission floor later"
        );
    }
    assert!(legacy.complete());
    assert!(!legacy.drained());

    let (mut relay, aggregates) = pending_aggregates(snapshot, true);
    for now in [-1, 39] {
        let before = relay.clone();
        assert!(relay.poll_write_at(snapshot.members[0].id, now).is_err());
        assert_eq!(relay, before);
    }
    for (recipient, aggregate) in &aggregates {
        relay.written(*recipient, aggregate.id).unwrap();
        relay
            .receive_at(
                *recipient,
                &RoomMessage::DrainReady {
                    participant: *recipient,
                    sequence: 1,
                },
                50,
            )
            .unwrap();
    }
    let own = snapshot.members[0].id;
    let notice = relay.poll_write_at(own, 60).unwrap().unwrap();
    relay.stop();
    relay.stop();
    assert!(!relay.active() && !relay.complete() && !relay.drained());
    let stopped = relay.clone();
    assert!(relay.written(own, notice.id).is_err());
    assert!(relay.poll_write_at(own, 61).is_err());
    assert!(
        relay
            .receive_at(
                own,
                &RoomMessage::DrainReady {
                    participant: own,
                    sequence: 1
                },
                61
            )
            .is_err()
    );
    assert_eq!(relay, stopped);
}
