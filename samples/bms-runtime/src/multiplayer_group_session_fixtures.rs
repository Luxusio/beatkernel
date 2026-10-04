//! Deferred paired-session fixtures; transport and browser execution are separate.
use crate::{
    local_players::PlayerId,
    multiplayer_group::{GroupPrefix, MemberProgress, encode_prefix},
    multiplayer_protocol::{
        FrameDecoder, GroupEvent, MultiplayerError, MultiplayerEvent, OutboundFrame, Progress,
        Session, WriteStep,
    },
    multiplayer_start::{StartPolicy, StartRole},
};

const IDENTITY: &[u8] = b"chart\0\xff";
const HOST: [PlayerId; 2] = [PlayerId(7), PlayerId(u32::MAX)];
const JOIN: [PlayerId; 1] = [PlayerId(91)];

fn policy() -> StartPolicy {
    StartPolicy {
        lead_ns: 10_000,
        min_remaining_ns: 100,
        max_age_ns: 100_000,
        max_uncertainty_ns: 100,
        max_release_lateness_ns: 25,
    }
}

fn owner(players: &[PlayerId], role: StartRole) -> Session {
    Session::new_group(IDENTITY.to_vec(), players.to_vec(), role, policy(), 0).unwrap()
}

fn frame(step: WriteStep) -> OutboundFrame {
    match step {
        WriteStep::Frame(frame) => frame,
        other => panic!("expected one actual frame, got {other:?}"),
    }
}

fn decode(bytes: &[u8]) -> (u8, Vec<u8>) {
    let mut decoder = FrameDecoder::new();
    for chunk in bytes.chunks(3) {
        let mut offset = 0;
        while offset < chunk.len() {
            let accepted = decoder.push(&chunk[offset..]).unwrap();
            assert!(accepted > 0);
            offset += accepted;
        }
    }
    decoder.take().unwrap().unwrap()
}

fn receive(receiver: &mut Session, outgoing: &OutboundFrame, now: i64) {
    let (tag, payload) = decode(&outgoing.bytes);
    receiver.receive(tag, &payload, now).unwrap();
}

fn drain(owner: &mut Session) -> Vec<MultiplayerEvent> {
    std::iter::from_fn(|| owner.poll_event()).collect()
}

fn groups(owner: &mut Session) -> Vec<GroupEvent> {
    std::iter::from_fn(|| owner.poll_group_event()).collect()
}

fn rows(players: &[PlayerId], song: i64, hits: u64) -> Vec<MemberProgress> {
    players
        .iter()
        .map(|&player| MemberProgress {
            player,
            progress: Progress {
                song_ns: song,
                hits,
                misses: 0,
                combo: hits,
                max_combo: hits,
            },
        })
        .collect()
}

fn fenced(owner: &mut Session, error: MultiplayerError, now: i64) {
    assert_eq!(owner.poll_write(now), Err(error.clone()));
    assert_eq!(owner.request_ready(), Err(error.clone()));
    assert_eq!(owner.written(1, now), Err(error.clone()));
    assert_eq!(owner.receive(1, IDENTITY, now), Err(error.clone()));
    assert_eq!(
        owner.send_group_progress(rows(&HOST, 0, 0), false, now),
        Err(error)
    );
}

struct Pair {
    host: Session,
    join: Session,
    now: i64,
    events: [Vec<MultiplayerEvent>; 2],
    groups: [Vec<GroupEvent>; 2],
    tags: [Vec<u8>; 2],
    last_ids: [u64; 2],
}

impl Pair {
    fn started(retain_join_events: bool) -> Self {
        let mut pair = Self {
            host: Session::new_group(
                IDENTITY.to_vec(),
                HOST.to_vec(),
                StartRole::Host,
                policy(),
                100,
            )
            .unwrap(),
            join: Session::new_group(
                IDENTITY.to_vec(),
                JOIN.to_vec(),
                StartRole::Join,
                policy(),
                200,
            )
            .unwrap(),
            now: 0,
            events: [vec![], vec![]],
            groups: [vec![], vec![]],
            tags: [vec![], vec![]],
            last_ids: [0, 0],
        };
        pair.host.request_ready().unwrap();
        pair.join.request_ready().unwrap();
        for _ in 0..128 {
            pair.now += 10;
            let host = pair.host.poll_write(pair.now).unwrap();
            let join = pair.join.poll_write(pair.now).unwrap();
            let mut pending = [None, None];
            for (index, step) in [host, join].into_iter().enumerate() {
                if let WriteStep::Frame(frame) = step {
                    assert!(frame.id > pair.last_ids[index]);
                    pair.last_ids[index] = frame.id;
                    pair.tags[index].push(decode(&frame.bytes).0);
                    pending[index] = Some(frame);
                }
            }
            if let Some(frame) = &pending[0] {
                pair.host.written(frame.id, pair.now + 1).unwrap();
            }
            if let Some(frame) = &pending[1] {
                pair.join.written(frame.id, pair.now + 1).unwrap();
            }
            if let Some(frame) = &pending[0] {
                receive(&mut pair.join, frame, pair.now + 2);
            }
            if let Some(frame) = &pending[1] {
                receive(&mut pair.host, frame, pair.now + 2);
            }
            pair.now += 2;
            pair.events[0].extend(drain(&mut pair.host));
            pair.groups[0].extend(groups(&mut pair.host));
            if !retain_join_events {
                pair.events[1].extend(drain(&mut pair.join));
                pair.groups[1].extend(groups(&mut pair.join));
            }
            if pair.host.start_committed() && pair.join.start_committed() {
                return pair;
            }
        }
        panic!("bounded real setup/probe/start exchange did not finish")
    }
}

#[test]
fn independent_rosters_need_complete_setup_and_ready_writes_before_any_probe() {
    let mut host = owner(&HOST, StartRole::Host);
    let remote: Vec<_> = (1..=64).map(PlayerId).collect();
    let mut join = owner(&remote, StartRole::Join);
    assert_eq!(host.local_roster(), Some(HOST.as_slice()));
    assert!(host.remote_roster().is_none());
    host.request_ready().unwrap();
    join.request_ready().unwrap();
    let host_setup = frame(host.poll_write(0).unwrap());
    let join_setup = frame(join.poll_write(0).unwrap());
    let (tag, payload) = decode(&host_setup.bytes);
    assert_eq!(tag, 14);
    assert_eq!(
        payload,
        [
            b'B', b'K', b'G', b'C', 1, 0, 2, 0, 7, 0, 0, 0, b'c', b'h', b'a', b'r', b't', 0, 0xff,
            7, 0, 0, 0, 0xff, 0xff, 0xff, 0xff,
        ]
    );
    // A transport prefix is not a received setup and carries no local credit.
    let mut partial = FrameDecoder::new();
    let half = host_setup.bytes.len() / 2;
    let mut consumed = 0;
    while consumed < half {
        let count = partial.push(&host_setup.bytes[consumed..half]).unwrap();
        assert!(count > 0);
        consumed += count;
    }
    assert!(partial.take().unwrap().is_none());
    assert!(!join.setup_complete());
    assert_eq!(host.poll_write(1).unwrap(), WriteStep::Waiting);
    receive(&mut host, &join_setup, 1);
    receive(&mut join, &host_setup, 1);
    assert_eq!(host.remote_roster(), Some(remote.as_slice()));
    assert_eq!(join.remote_roster(), Some(HOST.as_slice()));
    assert_eq!(drain(&mut host), vec![MultiplayerEvent::Connected]);
    assert_eq!(groups(&mut host), vec![GroupEvent::Roster(remote)]);
    assert_eq!(drain(&mut join), vec![MultiplayerEvent::Connected]);
    assert_eq!(groups(&mut join), vec![GroupEvent::Roster(HOST.to_vec())]);
    assert_eq!(host.poll_write(2).unwrap(), WriteStep::Waiting);
    host.written(host_setup.id, 2).unwrap();
    join.written(join_setup.id, 2).unwrap();
    let host_ready = frame(host.poll_write(3).unwrap());
    let join_ready = frame(join.poll_write(3).unwrap());
    assert_eq!(decode(&host_ready.bytes), (5, vec![]));
    receive(&mut host, &join_ready, 4);
    receive(&mut join, &host_ready, 4);
    assert!(drain(&mut host).is_empty());
    assert_eq!(host.poll_write(4).unwrap(), WriteStep::Waiting);
    host.written(host_ready.id, 4).unwrap();
    join.written(join_ready.id, 4).unwrap();
    assert_eq!(drain(&mut host), vec![MultiplayerEvent::Ready]);
    assert_eq!(drain(&mut join), vec![MultiplayerEvent::Ready]);
    assert_eq!(decode(&frame(host.poll_write(5).unwrap()).bytes).0, 6);
    assert!(!host.start_committed());
    assert!(host.preparation_pending());
}

#[test]
fn setup_extents_and_distinct_mode_tags_reject_ambiguous_or_malformed_envelopes() {
    let maximum: Vec<_> = (1..=64).map(PlayerId).collect();
    let identity = vec![0x61; 65_268];
    let mut sender = Session::new_group(
        identity.clone(),
        maximum.clone(),
        StartRole::Host,
        policy(),
        0,
    )
    .unwrap();
    let maximum_setup = frame(sender.poll_write(0).unwrap());
    let (tag, payload) = decode(&maximum_setup.bytes);
    assert_eq!(tag, 14);
    assert_eq!(payload.len(), 65_536);
    let mut receiver =
        Session::new_group(identity, JOIN.to_vec(), StartRole::Join, policy(), 0).unwrap();
    receiver.receive(tag, &payload, 0).unwrap();
    assert_eq!(receiver.remote_roster(), Some(maximum.as_slice()));
    assert!(Session::new_group(vec![0x61; 65_269], maximum, StartRole::Host, policy(), 0).is_err());
    for players in [
        vec![],
        vec![PlayerId(0)],
        vec![PlayerId(7), PlayerId(7)],
        (1..=65).map(PlayerId).collect(),
    ] {
        assert!(
            Session::new_group(IDENTITY.to_vec(), players, StartRole::Host, policy(), 0).is_err()
        );
    }
    assert!(Session::new_group(vec![], HOST.to_vec(), StartRole::Host, policy(), 0).is_err());

    let mut source = owner(&HOST, StartRole::Host);
    let group_setup = frame(source.poll_write(0).unwrap());
    let (_, body) = decode(&group_setup.bytes);
    let mut malformed = Vec::new();
    for (offset, value) in [
        (0, b'X'),
        (4, 0),
        (4, 2),
        (5, 1),
        (6, 0),
        (6, 65),
        (7, 1),
        (8, 0),
        (12, b'X'),
    ] {
        let mut bytes = body.clone();
        bytes[offset] = value;
        malformed.push(bytes);
    }
    let mut huge_identity = body.clone();
    huge_identity[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    malformed.push(huge_identity);
    for id in [0_u32, 7] {
        let mut bytes = body.clone();
        bytes[23..27].copy_from_slice(&id.to_le_bytes());
        malformed.push(bytes);
    }
    malformed.push(body[..11].to_vec());
    malformed.push(body[..body.len() - 1].to_vec());
    let mut trailing = body.clone();
    trailing.push(0);
    malformed.push(trailing);
    for bytes in malformed {
        let mut receiver = owner(&JOIN, StartRole::Join);
        let error = receiver.receive(14, &bytes, 0).unwrap_err();
        assert!(!receiver.setup_complete());
        assert!(receiver.remote_roster().is_none());
        assert!(drain(&mut receiver).is_empty());
        assert!(groups(&mut receiver).is_empty());
        fenced(&mut receiver, error, 0);
    }

    // Scalar identity is arbitrary bytes, including a complete BKGC envelope.
    let mut scalar = Session::new(body.clone(), StartRole::Join, policy(), 0).unwrap();
    let scalar_setup = frame(scalar.poll_write(0).unwrap());
    assert_eq!(decode(&scalar_setup.bytes), (1, body.clone()));
    let error = scalar.receive(14, &body, 0).unwrap_err();
    fenced(&mut scalar, error, 0);
    let mut group = owner(&HOST, StartRole::Host);
    let error = group.receive(1, &body, 0).unwrap_err();
    fenced(&mut group, error, 0);
    let mut ordinary = Session::new(body.clone(), StartRole::Host, policy(), 0).unwrap();
    ordinary.receive(1, &body, 0).unwrap();
    assert!(ordinary.setup_complete());
    assert!(ordinary.local_roster().is_none());
    assert!(ordinary.remote_roster().is_none());
    assert!(ordinary.poll_group_event().is_none());

    let mut prestart = owner(&JOIN, StartRole::Join);
    receive(&mut prestart, &group_setup, 0);
    let data = encode_prefix(0, false, &rows(&HOST, -1, 0)).unwrap();
    let error = prestart.receive(12, &data, 0).unwrap_err();
    assert_eq!(drain(&mut prestart), vec![MultiplayerEvent::Connected]);
    assert_eq!(
        groups(&mut prestart),
        vec![GroupEvent::Roster(HOST.to_vec())]
    );
    fenced(&mut prestart, error, 0);
}

#[test]
fn one_bilateral_start_and_exact_complete_writes_own_all_group_final_acknowledgements() {
    let mut pair = Pair::started(false);
    for index in 0..2 {
        assert_eq!(pair.tags[index].iter().filter(|&&tag| tag == 6).count(), 8);
        assert_eq!(pair.tags[index].iter().filter(|&&tag| tag == 7).count(), 8);
        assert_eq!(pair.events[index].len(), 4);
        assert_eq!(pair.events[index][0], MultiplayerEvent::Connected);
        assert_eq!(pair.events[index][1], MultiplayerEvent::Ready);
        assert!(matches!(
            pair.events[index][2],
            MultiplayerEvent::ClockEstimated(_)
        ));
    }
    let MultiplayerEvent::StartScheduled(host) = pair.events[0][3] else {
        panic!("missing host start")
    };
    let MultiplayerEvent::StartScheduled(join) = pair.events[1][3] else {
        panic!("missing join start")
    };
    assert_eq!(host.song_target_ns, join.song_target_ns);
    assert_eq!(host.target_ns + 100, host.song_target_ns);
    assert_eq!(join.target_ns + 200, join.song_target_ns);
    assert_eq!(pair.groups[0], vec![GroupEvent::Roster(JOIN.to_vec())]);
    assert_eq!(pair.groups[1], vec![GroupEvent::Roster(HOST.to_vec())]);
    let now = pair.now;
    assert_eq!(
        pair.host.poll_write(now).unwrap(),
        WriteStep::ApplicationSlot
    );
    let ordinary = rows(&HOST, i64::MIN, u64::MAX);
    let wire = pair
        .host
        .send_group_progress(ordinary.clone(), false, now)
        .unwrap();
    assert_eq!(decode(&wire.bytes).0, 12);
    let immutable = wire.bytes.clone();
    assert_eq!(pair.host.poll_write(now).unwrap(), WriteStep::Waiting);
    assert_eq!(wire.bytes, immutable);
    pair.host.written(wire.id, now).unwrap();
    receive(&mut pair.join, &wire, now);
    assert_eq!(
        groups(&mut pair.join),
        vec![GroupEvent::Progress(GroupPrefix {
            sequence: 0,
            final_prefix: false,
            members: ordinary,
        })]
    );
    assert!(drain(&mut pair.join).is_empty());
    let host_final = rows(&HOST, 604_800_000_000_000, u64::MAX);
    let join_final = vec![MemberProgress {
        player: JOIN[0],
        progress: Progress {
            song_ns: i64::MAX,
            hits: 0,
            misses: u64::MAX,
            combo: 0,
            max_combo: 0,
        },
    }];
    assert_eq!(
        pair.host.poll_write(now).unwrap(),
        WriteStep::ApplicationSlot
    );
    assert_eq!(
        pair.join.poll_write(now).unwrap(),
        WriteStep::ApplicationSlot
    );
    let host_wire = pair
        .host
        .send_group_progress(host_final.clone(), true, now)
        .unwrap();
    let join_wire = pair
        .join
        .send_group_progress(join_final.clone(), true, now)
        .unwrap();
    assert_eq!(decode(&host_wire.bytes).0, 13);
    receive(&mut pair.join, &host_wire, now);
    receive(&mut pair.host, &join_wire, now);
    assert_eq!(
        groups(&mut pair.host),
        vec![GroupEvent::Progress(GroupPrefix {
            sequence: 0,
            final_prefix: true,
            members: join_final
        })]
    );
    assert_eq!(
        groups(&mut pair.join),
        vec![GroupEvent::Progress(GroupPrefix {
            sequence: 1,
            final_prefix: true,
            members: host_final
        })]
    );
    assert!(drain(&mut pair.host).is_empty());
    assert!(drain(&mut pair.join).is_empty());
    assert_eq!(pair.host.poll_write(now).unwrap(), WriteStep::Waiting);
    pair.host.written(host_wire.id, now).unwrap();
    pair.join.written(join_wire.id, now).unwrap();
    let host_ack = frame(pair.host.poll_write(now).unwrap());
    let join_ack = frame(pair.join.poll_write(now).unwrap());
    assert_eq!(decode(&host_ack.bytes), (4, 0_u64.to_le_bytes().to_vec()));
    assert_eq!(decode(&join_ack.bytes), (4, 1_u64.to_le_bytes().to_vec()));
    receive(&mut pair.host, &join_ack, now);
    receive(&mut pair.join, &host_ack, now);
    assert!(drain(&mut pair.host).is_empty());
    assert!(drain(&mut pair.join).is_empty());
    pair.host.written(host_ack.id, now).unwrap();
    assert_eq!(
        drain(&mut pair.host),
        vec![MultiplayerEvent::FinalAcknowledged]
    );
    assert!(drain(&mut pair.join).is_empty());
    pair.join.written(join_ack.id, now).unwrap();
    assert_eq!(
        drain(&mut pair.join),
        vec![MultiplayerEvent::FinalAcknowledged]
    );
}

#[test]
fn changed_rosters_bad_transitions_mixed_data_and_stale_credit_fence_with_prior_events_intact() {
    for violation in 0..7 {
        let mut pair = Pair::started(false);
        let now = pair.now;
        assert_eq!(
            pair.host.poll_write(now).unwrap(),
            WriteStep::ApplicationSlot
        );
        let accepted = rows(&HOST, 10, 1);
        let first = pair
            .host
            .send_group_progress(accepted.clone(), false, now)
            .unwrap();
        pair.host.written(first.id, now).unwrap();
        receive(&mut pair.join, &first, now);
        let mut changed = rows(&HOST, 11, 2);
        let mut sequence = 1;
        let mut final_prefix = false;
        let mut tag = 12;
        match violation {
            0 => changed.swap(0, 1),
            1 => changed[1].player = PlayerId(92),
            2 => {
                changed[1].progress = Progress {
                    song_ns: 9,
                    hits: 2,
                    misses: 0,
                    combo: 2,
                    max_combo: 2,
                }
            }
            3 => sequence = 0,
            4 => final_prefix = true,
            5 => tag = 13,
            6 => tag = 2,
            _ => unreachable!(),
        }
        let payload = encode_prefix(sequence, final_prefix, &changed).unwrap();
        let error = pair.join.receive(tag, &payload, now).unwrap_err();
        assert_eq!(
            pair.join.remote_group_progress(),
            Some(&GroupPrefix {
                sequence: 0,
                final_prefix: false,
                members: accepted.clone(),
            })
        );
        assert_eq!(
            groups(&mut pair.join),
            vec![GroupEvent::Progress(GroupPrefix {
                sequence: 0,
                final_prefix: false,
                members: accepted,
            })]
        );
        assert_eq!(pair.join.remote_roster(), Some(HOST.as_slice()));
        fenced(&mut pair.join, error, now);
    }
    let mut outbound = Pair::started(false);
    let now = outbound.now;
    assert_eq!(
        outbound.host.poll_write(now).unwrap(),
        WriteStep::ApplicationSlot
    );
    let mut wrong_roster = rows(&HOST, 0, 0);
    wrong_roster.reverse();
    let error = outbound
        .host
        .send_group_progress(wrong_roster, false, now)
        .unwrap_err();
    fenced(&mut outbound.host, error, now);
    let mut mixed = Pair::started(false);
    let now = mixed.now;
    assert_eq!(
        mixed.host.poll_write(now).unwrap(),
        WriteStep::ApplicationSlot
    );
    let error = mixed
        .host
        .send_progress(rows(&HOST, 0, 0)[0].progress, false, now)
        .unwrap_err();
    fenced(&mut mixed.host, error, now);
    let mut scalar = Session::new(IDENTITY.to_vec(), StartRole::Host, policy(), 0).unwrap();
    let error = scalar
        .send_group_progress(rows(&HOST, 0, 0), false, 0)
        .unwrap_err();
    fenced(&mut scalar, error, 0);

    // A peer can receive a final frame before its sender receives write credit.
    // Even a genuine peer ACK cannot substitute for that missing local receipt.
    for invalid_credit in 0..3 {
        let mut pair = Pair::started(false);
        let now = pair.now;
        assert_eq!(
            pair.host.poll_write(now).unwrap(),
            WriteStep::ApplicationSlot
        );
        let final_frame = pair
            .host
            .send_group_progress(rows(&HOST, 0, 0), true, now)
            .unwrap();
        receive(&mut pair.join, &final_frame, now);
        let ack = frame(pair.join.poll_write(now).unwrap());
        pair.join.written(ack.id, now).unwrap();
        let (tag, mut payload) = decode(&ack.bytes);
        assert_eq!((tag, payload.clone()), (4, 0_u64.to_le_bytes().to_vec()));
        if invalid_credit != 0 {
            pair.host.written(final_frame.id, now).unwrap();
        }
        if invalid_credit == 1 {
            payload.copy_from_slice(&1_u64.to_le_bytes());
        }
        if invalid_credit == 2 {
            pair.host.receive(tag, &payload, now).unwrap();
        }
        let error = pair.host.receive(tag, &payload, now).unwrap_err();
        let accepted_events = drain(&mut pair.host);
        if invalid_credit == 2 {
            assert_eq!(accepted_events, vec![MultiplayerEvent::FinalAcknowledged]);
        } else {
            assert!(accepted_events.is_empty());
        }
        assert_eq!(
            pair.host.local_group_progress(),
            Some(&GroupPrefix {
                sequence: 0,
                final_prefix: true,
                members: rows(&HOST, 0, 0),
            })
        );
        fenced(&mut pair.host, error, now);
    }
    let mut duplicate_write = Pair::started(false);
    let now = duplicate_write.now;
    assert_eq!(
        duplicate_write.host.poll_write(now).unwrap(),
        WriteStep::ApplicationSlot
    );
    let outgoing = duplicate_write
        .host
        .send_group_progress(rows(&HOST, 0, 0), false, now)
        .unwrap();
    duplicate_write.host.written(outgoing.id, now).unwrap();
    let error = duplicate_write.host.written(outgoing.id, now).unwrap_err();
    fenced(&mut duplicate_write.host, error, now);
}

#[test]
fn lifecycle_and_group_events_share_one_eight_event_budget_without_dropping_the_prefix() {
    let mut pair = Pair::started(true);
    let now = pair.now;
    let mut expected = vec![GroupEvent::Roster(HOST.to_vec())];
    // Four undrained lifecycle events plus the roster leave three group slots.
    for index in 0..4_u64 {
        assert_eq!(
            pair.host.poll_write(now).unwrap(),
            WriteStep::ApplicationSlot
        );
        let members = rows(&HOST, index as i64, index);
        let outgoing = pair
            .host
            .send_group_progress(members.clone(), false, now)
            .unwrap();
        pair.host.written(outgoing.id, now).unwrap();
        let (tag, payload) = decode(&outgoing.bytes);
        let result = pair.join.receive(tag, &payload, now);
        if index < 3 {
            result.unwrap();
            expected.push(GroupEvent::Progress(GroupPrefix {
                sequence: index,
                final_prefix: false,
                members,
            }));
        } else {
            assert_eq!(result, Err(MultiplayerError::QueueFull));
        }
    }
    assert_eq!(
        pair.join.remote_group_progress(),
        Some(&GroupPrefix {
            sequence: 2,
            final_prefix: false,
            members: rows(&HOST, 2, 2),
        })
    );
    assert_eq!(groups(&mut pair.join), expected);
    let lifecycle = drain(&mut pair.join);
    assert_eq!(lifecycle.len(), 4);
    assert_eq!(lifecycle[0], MultiplayerEvent::Connected);
    assert_eq!(lifecycle[1], MultiplayerEvent::Ready);
    assert!(matches!(lifecycle[2], MultiplayerEvent::ClockEstimated(_)));
    assert!(matches!(lifecycle[3], MultiplayerEvent::StartScheduled(_)));
    fenced(&mut pair.join, MultiplayerError::QueueFull, now);
    assert!(pair.join.poll_event().is_none());
    assert!(pair.join.poll_group_event().is_none());
}
