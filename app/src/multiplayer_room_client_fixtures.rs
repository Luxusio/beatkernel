//! Deferred client/session fixtures. Scripted byte I/O is not endpoint acceptance.
use super::*;
use crate::{
    local_players::PlayerId,
    multiplayer_group_rooms::{
        GroupRoomMember, GroupRoomPhase, GroupRoomPolicy, GroupRoomRegistry,
    },
    multiplayer_room_wire::{RoomMessage, decode_message, encode_message},
    multiplayer_rooms::ParticipantId,
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    io::{self, Read, Write},
    rc::Rc,
};

const IDENTITY: &[u8] = &[0, 255, 17];
const PLAYERS: &[PlayerId] = &[PlayerId(u32::MAX), PlayerId(0x0102_0304)];
const SELF: ParticipantId = ParticipantId(0x8877_6655_4433_2211);
const OTHER: ParticipantId = ParticipantId(u64::MAX);

fn client() -> RoomClientSession {
    RoomClientSession::new(IDENTITY, PLAYERS).unwrap()
}

fn host(id: ParticipantId) -> GroupRoomMember {
    GroupRoomMember {
        id,
        players: PLAYERS.to_vec(),
        prepared: false,
    }
}

fn snapshot(members: Vec<GroupRoomMember>, phase: GroupRoomPhase) -> RoomMessage {
    RoomMessage::Snapshot {
        members,
        phase,
        deadline_ns: if phase == GroupRoomPhase::Prepared {
            None
        } else {
            Some(i64::MAX)
        },
    }
}

fn retained(session: &RoomClientSession) -> RoomMessage {
    let room = session.room().unwrap();
    assert_eq!(room.identity, IDENTITY);
    RoomMessage::Snapshot {
        members: room.members.to_vec(),
        phase: room.phase,
        deadline_ns: room.deadline_ns,
    }
}

fn completed_write(session: &mut RoomClientSession, expected: RoomMessage) -> u64 {
    let frame = session.poll_write().unwrap().unwrap();
    assert_eq!(decode_message(&frame.bytes).unwrap(), expected);
    assert!(session.poll_write().unwrap().is_none());
    session.written(frame.id).unwrap();
    frame.id
}

fn admitted(creator: bool) -> RoomClientSession {
    let mut session = client();
    completed_write(
        &mut session,
        RoomMessage::Join {
            identity: IDENTITY.to_vec(),
            players: PLAYERS.to_vec(),
        },
    );
    session
        .receive(RoomMessage::Admitted { participant: SELF })
        .unwrap();
    let members = if creator {
        vec![host(SELF), host(OTHER)]
    } else {
        vec![host(OTHER), host(SELF)]
    };
    session
        .receive(snapshot(members, GroupRoomPhase::Collecting))
        .unwrap();
    session
}

fn registry_snapshot(registry: &GroupRoomRegistry) -> RoomMessage {
    let room = registry.room("room").unwrap();
    let message = RoomMessage::Snapshot {
        members: room.members.to_vec(),
        phase: room.phase,
        deadline_ns: room.deadline_ns,
    };
    decode_message(&encode_message(&message).unwrap()).unwrap()
}

#[test]
fn literal_join_owned_inputs_and_exact_complete_receipts_preserve_full_width_ids() {
    let mut identity = IDENTITY.to_vec();
    let mut players = PLAYERS.to_vec();
    let mut session = RoomClientSession::new(&identity, &players).unwrap();
    identity.fill(7);
    players.fill(PlayerId(7));
    assert_eq!(session.participant(), None);
    assert!(session.room().is_none());
    assert!(!session.leave_written());
    assert!(
        session
            .receive(RoomMessage::Admitted { participant: SELF })
            .is_err()
    );
    assert!(session.written(1).is_err());
    let join = session.poll_write().unwrap().unwrap();
    assert_eq!(join.id, 1);
    assert_eq!(
        join.bytes,
        [
            b'B', b'K', b'M', b'R', 2, 0, 1, 16, 0, 0, 0, 3, 0, 0, 0, 0, 255, 17, 2, 255, 255, 255,
            255, 4, 3, 2, 1,
        ]
    );
    assert!(
        session
            .receive(RoomMessage::Admitted { participant: SELF })
            .is_err()
    );
    assert!(session.written(0).is_err());
    assert!(session.written(2).is_err());
    session.written(join.id).unwrap();
    assert!(session.written(join.id).is_err());
    assert!(
        session
            .receive(RoomMessage::Admitted {
                participant: ParticipantId(0)
            })
            .is_err()
    );
    session
        .receive(RoomMessage::Admitted { participant: SELF })
        .unwrap();
    assert_eq!(session.participant(), Some(SELF));
    assert!(
        session
            .receive(RoomMessage::Admitted { participant: OTHER })
            .is_err()
    );
    session
        .receive(snapshot(vec![host(SELF)], GroupRoomPhase::Collecting))
        .unwrap();
    assert_eq!(session.room().unwrap().members[0].players, PLAYERS);

    let mut last = client();
    last.next_id = Some(u64::MAX);
    assert_eq!(
        completed_write(
            &mut last,
            RoomMessage::Join {
                identity: IDENTITY.to_vec(),
                players: PLAYERS.to_vec()
            }
        ),
        u64::MAX
    );
    last.receive(RoomMessage::Admitted { participant: SELF })
        .unwrap();
    last.receive(snapshot(vec![host(SELF)], GroupRoomPhase::Collecting))
        .unwrap();
    last.request_leave().unwrap();
    assert_eq!(last.poll_write().unwrap_err(), RoomClientError::IdExhausted);
    assert!(!last.leave_written());
    assert!(last.written(u64::MAX).is_err());

    for (identity, players) in [
        (vec![], vec![PlayerId(1)]),
        (vec![1; 65_537], vec![PlayerId(1)]),
        (vec![1], vec![]),
        (vec![1], vec![PlayerId(0)]),
        (vec![1], vec![PlayerId(7), PlayerId(7)]),
        (vec![1], (1..=65).map(PlayerId).collect()),
    ] {
        assert!(RoomClientSession::new(&identity, &players).is_err());
    }
}

#[test]
fn actual_registry_snapshots_append_then_freeze_two_three_four_sixty_four_scoped_rosters() {
    for count in [2usize, 3, 4, 64] {
        let players = (0..64)
            .map(|index| PlayerId(u32::MAX - index))
            .collect::<Vec<_>>();
        let identity = vec![0xA5; 65_536];
        let mut session = RoomClientSession::new(&identity, &players).unwrap();
        completed_write(
            &mut session,
            RoomMessage::Join {
                identity: identity.clone(),
                players: players.clone(),
            },
        );
        let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, count, 8, 100).unwrap());
        let creator = registry.join("room", &identity, &players, 0).unwrap();
        session
            .receive(RoomMessage::Admitted {
                participant: creator.id,
            })
            .unwrap();
        session.receive(registry_snapshot(&registry)).unwrap();
        let mut tickets = vec![creator];
        for index in 1..count {
            tickets.push(
                registry
                    .join("room", &identity, &players, index as i64)
                    .unwrap(),
            );
            session.receive(registry_snapshot(&registry)).unwrap();
            assert_eq!(session.room().unwrap().members.len(), index + 1);
        }
        assert_eq!(session.room().unwrap().deadline_ns, Some(100));
        session.request_seal().unwrap();
        completed_write(&mut session, RoomMessage::Seal);
        registry.seal(tickets[0].id, 64).unwrap();
        session.receive(registry_snapshot(&registry)).unwrap();
        session.request_ready().unwrap();
        completed_write(&mut session, RoomMessage::Ready);
        for ticket in &tickets {
            registry.ready(ticket.id, 65).unwrap();
            session.receive(registry_snapshot(&registry)).unwrap();
        }
        let room = session.room().unwrap();
        assert_eq!(room.identity, identity);
        assert_eq!(room.phase, GroupRoomPhase::Prepared);
        assert_eq!(room.deadline_ns, None);
        for (member, ticket) in room.members.iter().zip(&tickets) {
            assert_eq!(member.id, ticket.id);
            assert_eq!(member.players, players);
            assert!(member.prepared);
        }
        session.receive(registry_snapshot(&registry)).unwrap();
        assert!(
            !session.leave_written(),
            "room preparation is not a Leave receipt or a gameplay start"
        );
    }
}

#[test]
fn malformed_later_members_and_phase_changes_leave_the_entire_accepted_snapshot_intact() {
    let mut session = admitted(true);
    let before = retained(&session);
    let mut invalid = Vec::new();
    invalid.push(snapshot(vec![host(SELF)], GroupRoomPhase::Collecting));
    invalid.push(snapshot(
        vec![host(OTHER), host(SELF)],
        GroupRoomPhase::Collecting,
    ));
    invalid.push(snapshot(
        vec![host(SELF), host(SELF)],
        GroupRoomPhase::Collecting,
    ));
    invalid.push(snapshot(
        vec![host(SELF), host(ParticipantId(0))],
        GroupRoomPhase::Collecting,
    ));
    for bad_players in [
        vec![PlayerId(0)],
        vec![PlayerId(7), PlayerId(7)],
        vec![PlayerId(7)],
    ] {
        let mut later = host(OTHER);
        later.players = bad_players;
        invalid.push(snapshot(
            vec![host(SELF), later],
            GroupRoomPhase::Collecting,
        ));
    }
    let mut changed_self = host(SELF);
    changed_self.players.reverse();
    invalid.push(snapshot(
        vec![changed_self, host(OTHER)],
        GroupRoomPhase::Collecting,
    ));
    for deadline_ns in [None, Some(-1), Some(i64::MAX - 1)] {
        invalid.push(RoomMessage::Snapshot {
            members: vec![host(SELF), host(OTHER)],
            phase: GroupRoomPhase::Collecting,
            deadline_ns,
        });
    }
    invalid.push(snapshot(
        vec![host(SELF), host(OTHER)],
        GroupRoomPhase::Frozen,
    ));
    let prepared = vec![
        GroupRoomMember {
            prepared: true,
            ..host(SELF)
        },
        GroupRoomMember {
            prepared: true,
            ..host(OTHER)
        },
    ];
    invalid.push(snapshot(prepared, GroupRoomPhase::Prepared));
    for message in invalid {
        assert!(session.receive(message).is_err());
        assert_eq!(retained(&session), before);
        assert_eq!(session.participant(), Some(SELF));
    }
    session.request_seal().unwrap();
    completed_write(&mut session, RoomMessage::Seal);
    session
        .receive(snapshot(
            vec![host(SELF), host(OTHER)],
            GroupRoomPhase::Frozen,
        ))
        .unwrap();
    let frozen = retained(&session);
    for message in [
        before,
        snapshot(
            vec![host(SELF), host(OTHER), host(ParticipantId(7))],
            GroupRoomPhase::Frozen,
        ),
        snapshot(vec![host(OTHER), host(SELF)], GroupRoomPhase::Frozen),
        snapshot(
            vec![
                GroupRoomMember {
                    prepared: true,
                    ..host(SELF)
                },
                host(OTHER),
            ],
            GroupRoomPhase::Frozen,
        ),
    ] {
        assert!(session.receive(message).is_err());
        assert_eq!(retained(&session), frozen);
    }
    session
        .receive(snapshot(
            vec![
                host(SELF),
                GroupRoomMember {
                    prepared: true,
                    ..host(OTHER)
                },
            ],
            GroupRoomPhase::Frozen,
        ))
        .unwrap();
    let other_ready = retained(&session);
    assert!(session.receive(frozen).is_err());
    assert_eq!(retained(&session), other_ready);
    session.request_ready().unwrap();
    completed_write(&mut session, RoomMessage::Ready);
    session
        .receive(snapshot(
            vec![
                GroupRoomMember {
                    prepared: true,
                    ..host(SELF)
                },
                GroupRoomMember {
                    prepared: true,
                    ..host(OTHER)
                },
            ],
            GroupRoomPhase::Prepared,
        ))
        .unwrap();
    let prepared = retained(&session);
    assert!(session.receive(other_ready).is_err());
    assert_eq!(retained(&session), prepared);
}

#[test]
fn request_roles_busy_and_write_receipts_gate_sealing_preparation_and_terminal_leave() {
    let mut session = client();
    for request in [
        RoomClientSession::request_seal,
        RoomClientSession::request_ready,
        RoomClientSession::request_leave,
    ] {
        assert!(
            request(&mut session).is_err(),
            "queued Join already owns the sole write slot"
        );
    }
    completed_write(
        &mut session,
        RoomMessage::Join {
            identity: IDENTITY.to_vec(),
            players: PLAYERS.to_vec(),
        },
    );
    session
        .receive(RoomMessage::Admitted { participant: SELF })
        .unwrap();
    session
        .receive(snapshot(vec![host(SELF)], GroupRoomPhase::Collecting))
        .unwrap();
    assert!(session.request_seal().is_err());
    assert!(session.request_ready().is_err());
    session
        .receive(snapshot(
            vec![host(SELF), host(OTHER)],
            GroupRoomPhase::Collecting,
        ))
        .unwrap();
    session.request_seal().unwrap();
    assert!(session.request_leave().is_err());
    let seal = session.poll_write().unwrap().unwrap();
    assert!(session.request_seal().is_err());
    assert!(session.request_ready().is_err());
    assert!(
        session
            .receive(snapshot(
                vec![host(SELF), host(OTHER)],
                GroupRoomPhase::Frozen
            ))
            .is_err()
    );
    session.written(seal.id).unwrap();
    session
        .receive(snapshot(
            vec![host(SELF), host(OTHER)],
            GroupRoomPhase::Frozen,
        ))
        .unwrap();
    assert!(session.request_seal().is_err());
    session.request_ready().unwrap();
    let ready = session.poll_write().unwrap().unwrap();
    assert!(ready.id > seal.id);
    let prepared_self = snapshot(
        vec![
            GroupRoomMember {
                prepared: true,
                ..host(SELF)
            },
            host(OTHER),
        ],
        GroupRoomPhase::Frozen,
    );
    assert!(session.receive(prepared_self.clone()).is_err());
    session.written(ready.id).unwrap();
    session.receive(prepared_self).unwrap();
    assert!(session.request_ready().is_err());
    session.request_leave().unwrap();
    assert!(!session.leave_written());
    assert!(session.request_ready().is_err());
    let leave = session.poll_write().unwrap().unwrap();
    assert!(leave.id > ready.id);
    assert_eq!(decode_message(&leave.bytes).unwrap(), RoomMessage::Leave);
    assert!(session.written(leave.id - 1).is_err());
    assert!(!session.leave_written());
    session.written(leave.id).unwrap();
    assert!(session.leave_written());
    assert!(session.request_leave().is_err());
    assert!(session.request_seal().is_err());
    assert!(session.request_ready().is_err());
    assert!(session.poll_write().unwrap().is_none());

    let mut follower = admitted(false);
    assert!(follower.request_seal().is_err());
    follower
        .receive(snapshot(
            vec![host(OTHER), host(SELF)],
            GroupRoomPhase::Frozen,
        ))
        .unwrap();
    follower.request_ready().unwrap();
    completed_write(&mut follower, RoomMessage::Ready);
    follower
        .receive(snapshot(
            vec![
                GroupRoomMember {
                    prepared: true,
                    ..host(OTHER)
                },
                GroupRoomMember {
                    prepared: true,
                    ..host(SELF)
                },
            ],
            GroupRoomPhase::Prepared,
        ))
        .unwrap();
}

enum ReadPart {
    Bytes(Vec<u8>),
    Blocked,
    Eof,
    Overcount,
    Error(io::ErrorKind),
}
enum WritePart {
    Bytes(usize),
    Blocked,
    Zero,
    Overcount,
    Error(io::ErrorKind),
}

#[derive(Default)]
struct Script {
    reads: VecDeque<ReadPart>,
    writes: VecDeque<WritePart>,
    output: Vec<u8>,
    read_calls: usize,
    write_calls: usize,
}

#[derive(Clone)]
struct ByteStream(Rc<RefCell<Script>>);

impl Read for ByteStream {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        let mut script = self.0.borrow_mut();
        script.read_calls += 1;
        match script.reads.pop_front().unwrap_or(ReadPart::Blocked) {
            ReadPart::Bytes(bytes) => {
                let count = dst.len().min(bytes.len());
                dst[..count].copy_from_slice(&bytes[..count]);
                if count < bytes.len() {
                    script
                        .reads
                        .push_front(ReadPart::Bytes(bytes[count..].to_vec()));
                }
                Ok(count)
            }
            ReadPart::Blocked => Err(io::ErrorKind::WouldBlock.into()),
            ReadPart::Eof => Ok(0),
            ReadPart::Overcount => Ok(dst.len() + 1),
            ReadPart::Error(kind) => Err(kind.into()),
        }
    }
}

impl Write for ByteStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut script = self.0.borrow_mut();
        script.write_calls += 1;
        match script.writes.pop_front().unwrap_or(WritePart::Blocked) {
            WritePart::Bytes(limit) => {
                let count = bytes.len().min(limit);
                script.output.extend_from_slice(&bytes[..count]);
                Ok(count)
            }
            WritePart::Blocked => Err(io::ErrorKind::WouldBlock.into()),
            WritePart::Zero => Ok(0),
            WritePart::Overcount => Ok(bytes.len() + 1),
            WritePart::Error(kind) => Err(kind.into()),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn driver() -> (RoomClientIo<ByteStream>, Rc<RefCell<Script>>) {
    let script = Rc::new(RefCell::new(Script::default()));
    (
        RoomClientIo::new(client(), ByteStream(script.clone())),
        script,
    )
}

fn bounded_step(
    driver: &mut RoomClientIo<ByteStream>,
    script: &Rc<RefCell<Script>>,
) -> io::Result<bool> {
    let before = {
        let s = script.borrow();
        (s.write_calls, s.read_calls)
    };
    let result = driver.step();
    let s = script.borrow();
    assert!(s.write_calls - before.0 <= 1);
    assert!(s.read_calls - before.1 <= 1);
    result
}

#[test]
fn actual_io_driver_keeps_partial_offsets_and_consumes_coalesced_frames_in_order() {
    let (mut driver, script) = driver();
    let join = encode_message(&RoomMessage::Join {
        identity: IDENTITY.to_vec(),
        players: PLAYERS.to_vec(),
    })
    .unwrap();
    assert!(driver.request_ready().is_err());
    script.borrow_mut().writes.extend([
        WritePart::Bytes(3),
        WritePart::Blocked,
        WritePart::Error(io::ErrorKind::Interrupted),
        WritePart::Bytes(usize::MAX),
    ]);
    assert!(bounded_step(&mut driver, &script).unwrap());
    assert_eq!(script.borrow().output, join[..3]);
    assert!(!bounded_step(&mut driver, &script).unwrap());
    assert_eq!(script.borrow().output, join[..3]);
    assert!(!bounded_step(&mut driver, &script).unwrap());
    assert_eq!(script.borrow().output, join[..3]);
    assert!(bounded_step(&mut driver, &script).unwrap());
    assert_eq!(script.borrow().output, join);
    assert_eq!(driver.session().participant(), None);
    let admitted = encode_message(&RoomMessage::Admitted { participant: SELF }).unwrap();
    let collecting = snapshot(vec![host(SELF), host(OTHER)], GroupRoomPhase::Collecting);
    let mut incoming = admitted;
    incoming.extend(encode_message(&collecting).unwrap());
    script.borrow_mut().reads.extend([
        ReadPart::Bytes(incoming[..3].to_vec()),
        ReadPart::Blocked,
        ReadPart::Error(io::ErrorKind::Interrupted),
        ReadPart::Bytes(incoming[3..].to_vec()),
    ]);
    for _ in 0..incoming.len() + 2 {
        bounded_step(&mut driver, &script).unwrap();
        if driver.session().room().is_some() {
            break;
        }
    }
    assert_eq!(driver.session().participant(), Some(SELF));
    assert_eq!(retained(driver.session()), collecting);
    assert_eq!(
        script.borrow().output,
        join,
        "no request or receipt is fabricated during reads"
    );
    driver.request_seal().unwrap();
    assert!(driver.request_ready().is_err());
    script
        .borrow_mut()
        .writes
        .push_back(WritePart::Bytes(usize::MAX));
    bounded_step(&mut driver, &script).unwrap();
    assert_eq!(
        decode_message(&script.borrow().output[join.len()..]).unwrap(),
        RoomMessage::Seal
    );
    let frozen = snapshot(vec![host(SELF), host(OTHER)], GroupRoomPhase::Frozen);
    script
        .borrow_mut()
        .reads
        .push_back(ReadPart::Bytes(encode_message(&frozen).unwrap()));
    for _ in 0..4 {
        bounded_step(&mut driver, &script).unwrap();
    }
    assert_eq!(retained(driver.session()), frozen);
    driver.request_leave().unwrap();
    script.borrow_mut().writes.push_back(WritePart::Bytes(1));
    bounded_step(&mut driver, &script).unwrap();
    assert!(!driver.session().leave_written());
    script
        .borrow_mut()
        .writes
        .push_back(WritePart::Bytes(usize::MAX));
    bounded_step(&mut driver, &script).unwrap();
    assert!(driver.session().leave_written());
    let stream = driver.into_stream();
    assert!(
        Rc::ptr_eq(&stream.0, &script),
        "caller retains actual stream teardown ownership"
    );
}

#[test]
fn eof_write_zero_transport_and_protocol_failures_fence_without_late_receipt_or_retry() {
    for failure in 0..9 {
        let (mut driver, script) = driver();
        match failure {
            0 => {
                script.borrow_mut().writes.push_back(WritePart::Zero);
            }
            1 => {
                script
                    .borrow_mut()
                    .writes
                    .push_back(WritePart::Error(io::ErrorKind::BrokenPipe));
            }
            2 => {
                script.borrow_mut().reads.push_back(ReadPart::Eof);
            }
            3 => {
                script
                    .borrow_mut()
                    .reads
                    .push_back(ReadPart::Error(io::ErrorKind::ConnectionReset));
            }
            4 => {
                script
                    .borrow_mut()
                    .writes
                    .push_back(WritePart::Bytes(usize::MAX));
                let mut bad = encode_message(&RoomMessage::Admitted { participant: SELF }).unwrap();
                bad[0] = b'X';
                script.borrow_mut().reads.push_back(ReadPart::Bytes(bad));
            }
            5 => {
                // An Admitted frame is invalid while the actual Join remains unwritten.
                script.borrow_mut().reads.push_back(ReadPart::Bytes(
                    encode_message(&RoomMessage::Admitted { participant: SELF }).unwrap(),
                ));
            }
            6 => {
                script
                    .borrow_mut()
                    .writes
                    .push_back(WritePart::Bytes(usize::MAX));
                script
                    .borrow_mut()
                    .reads
                    .extend([ReadPart::Bytes(b"BK".to_vec()), ReadPart::Eof]);
            }
            7 => {
                script.borrow_mut().writes.push_back(WritePart::Overcount);
            }
            _ => {
                script.borrow_mut().reads.push_back(ReadPart::Overcount);
            }
        }
        let mut terminal = None;
        for _ in 0..32 {
            if let Err(error) = bounded_step(&mut driver, &script) {
                terminal = Some(error);
                break;
            }
        }
        let error = terminal.expect("bounded malformed input must fail");
        if failure >= 7 {
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        }
        let original = (error.kind(), error.to_string());
        let before = {
            let s = script.borrow();
            (s.output.clone(), s.write_calls, s.read_calls)
        };
        script
            .borrow_mut()
            .writes
            .push_back(WritePart::Bytes(usize::MAX));
        script.borrow_mut().reads.push_back(ReadPart::Bytes(
            encode_message(&RoomMessage::Admitted { participant: SELF }).unwrap(),
        ));
        for error in [
            driver.step().unwrap_err(),
            driver.request_leave().unwrap_err(),
            driver.request_seal().unwrap_err(),
            driver.request_ready().unwrap_err(),
        ] {
            assert_eq!((error.kind(), error.to_string()), original);
        }
        let s = script.borrow();
        assert_eq!((s.output.clone(), s.write_calls, s.read_calls), before);
        assert_eq!(driver.session().participant(), None);
        assert!(driver.session().room().is_none());
        assert!(!driver.session().leave_written());
    }

    let session = admitted(false);
    let before = retained(&session);
    let script = Rc::new(RefCell::new(Script::default()));
    let mut driver = RoomClientIo::new(session, ByteStream(script.clone()));
    let mut changed = host(SELF);
    changed.players = vec![PlayerId(7)];
    let malformed_later = snapshot(vec![host(OTHER), changed], GroupRoomPhase::Frozen);
    // This is a valid wire frame but contradicts the admitted client's local roster.
    script
        .borrow_mut()
        .reads
        .push_back(ReadPart::Bytes(encode_message(&malformed_later).unwrap()));
    let mut terminal = None;
    for _ in 0..4 {
        if let Err(error) = bounded_step(&mut driver, &script) {
            terminal = Some(error);
            break;
        }
    }
    assert_eq!(terminal.unwrap().kind(), io::ErrorKind::InvalidData);
    assert_eq!(retained(driver.session()), before);
    assert_eq!(driver.session().participant(), Some(SELF));
    script.borrow_mut().reads.push_back(ReadPart::Bytes(
        encode_message(&snapshot(
            vec![host(OTHER), host(SELF)],
            GroupRoomPhase::Frozen,
        ))
        .unwrap(),
    ));
    let reads = script.borrow().read_calls;
    assert!(driver.step().is_err());
    assert_eq!(script.borrow().read_calls, reads);
    assert_eq!(retained(driver.session()), before);
}

#[test]
fn admission_only_client_refuses_clock_start_controls_without_changing_prepared_ownership() {
    use crate::multiplayer_start::StartMessage;
    let mut session = admitted(false);
    session
        .receive(snapshot(
            vec![host(OTHER), host(SELF)],
            GroupRoomPhase::Frozen,
        ))
        .unwrap();
    session.request_ready().unwrap();
    completed_write(&mut session, RoomMessage::Ready);
    session
        .receive(snapshot(
            vec![
                GroupRoomMember {
                    prepared: true,
                    ..host(OTHER)
                },
                GroupRoomMember {
                    prepared: true,
                    ..host(SELF)
                },
            ],
            GroupRoomPhase::Prepared,
        ))
        .unwrap();
    let retained_snapshot = retained(&session);
    for message in [
        RoomMessage::ClockPing {
            sequence: u64::MAX,
            sent_ns: 0,
        },
        RoomMessage::ClockPong {
            sequence: u64::MAX,
            sent_ns: i64::MAX,
            received_ns: 0,
            replied_ns: 0,
        },
        RoomMessage::Start(StartMessage::ClockReady(0)),
        RoomMessage::Start(StartMessage::Propose(i64::MAX)),
        RoomMessage::Start(StartMessage::Accept(i64::MAX)),
        RoomMessage::Start(StartMessage::Commit(i64::MAX)),
    ] {
        let bytes = encode_message(&message).unwrap();
        assert_eq!(u16::from_le_bytes(bytes[4..6].try_into().unwrap()), 2);
        assert_eq!(
            session.receive(decode_message(&bytes).unwrap()),
            Err(RoomClientError::InvalidState)
        );
        assert_eq!(session.participant(), Some(SELF));
        assert_eq!(retained(&session), retained_snapshot);
        assert_eq!(session.poll_write().unwrap(), None);
        assert!(!session.leave_written());
    }
    session.request_leave().unwrap();
    assert_eq!(completed_write(&mut session, RoomMessage::Leave), 3);
    assert!(session.leave_written());
}
