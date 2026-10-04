//! Deferred source fixtures: actual facade channels and Session transitions, no sockets.
use super::*;
use crate::{
    local_players::PlayerId,
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_protocol::{GroupEvent, OutboundFrame},
    multiplayer_start::StartRole,
};

const PLAYERS: [PlayerId; 2] = [PlayerId(7), PlayerId(u32::MAX)];
const REMOTE: [PlayerId; 1] = [PlayerId(91)];
const IDENTITY: &[u8] = b"native-group-fixture\0\xff";

fn members(players: &[PlayerId], song_ns: i64, hits: u64) -> Vec<MemberProgress> {
    players
        .iter()
        .map(|&player| MemberProgress {
            player,
            progress: Progress {
                song_ns,
                hits,
                misses: 0,
                combo: hits,
                max_combo: hits,
            },
        })
        .collect()
}

struct Channels {
    outgoing: Receiver<OutgoingMessage>,
    incoming: SyncSender<MultiplayerNotice>,
    terminal: SyncSender<MultiplayerError>,
}

fn channel_owner(players: Option<&[PlayerId]>, capacity: usize) -> (Multiplayer, Channels) {
    let (outgoing, out_rx) = mpsc::sync_channel(capacity);
    let (in_tx, incoming) = mpsc::sync_channel(16);
    let (terminal_tx, terminal) = mpsc::sync_channel(1);
    (
        Multiplayer {
            outgoing,
            incoming,
            terminal,
            stop_flag: Arc::new(AtomicBool::new(false)),
            ready_requested: Arc::new(AtomicBool::new(false)),
            ready: false,
            clock_epoch: Instant::now(),
            clock_estimate: None,
            start_schedule: None,
            start_policy: StartPolicy::default(),
            worker: None,
            local: None,
            remote: None,
            closed: false,
            connected: false,
            local_final: false,
            remote_final: None,
            final_acknowledged: false,
            finish_timeout: Duration::ZERO,
            group: players.map(|players| GroupOwnerState {
                local_roster: players.to_vec(),
                local: None,
                remote_roster: None,
                remote: None,
                remote_final: None,
            }),
        },
        Channels {
            outgoing: out_rx,
            incoming: in_tx,
            terminal: terminal_tx,
        },
    )
}

fn front(players: &[PlayerId], capacity: usize) -> (GroupMultiplayer, Channels) {
    let (owner, channels) = channel_owner(Some(players), capacity);
    (GroupMultiplayer { owner }, channels)
}

fn ready(front: &mut GroupMultiplayer, channels: &Channels) {
    let schedule = StartSchedule {
        target_ns: 2_000_000_000,
        song_target_ns: 2_100_000_000,
        uncertainty_ns: 0,
    };
    for event in [
        MultiplayerEvent::Connected,
        MultiplayerEvent::Ready,
        MultiplayerEvent::StartScheduled(schedule),
    ] {
        channels
            .incoming
            .try_send(MultiplayerNotice::Session(event))
            .unwrap();
    }
    assert_eq!(front.poll().len(), 3);
    assert!(front.is_connected() && front.is_ready());
    assert_eq!(front.start_schedule(), Some(schedule));
}

fn accepted(front: &GroupMultiplayer) -> Option<&[MemberProgress]> {
    front.owner.group.as_ref().unwrap().local.as_deref()
}

fn group_message(message: OutgoingMessage) -> (Vec<MemberProgress>, bool) {
    match message {
        OutgoingMessage::Group {
            members,
            final_prefix,
        } => (members, final_prefix),
        OutgoingMessage::Scalar(_) => panic!("group publication became a scalar alias"),
    }
}

#[test]
fn constructors_preflight_rosters_and_options_before_endpoint_ownership_and_admit_full_width_rows()
{
    let address = SocketAddr::from(([127, 0, 0, 1], 0));
    let connection = WebTransportOptions {
        url: "https://localhost:4433/rooms/cohort".into(),
        origin: "https://localhost".into(),
        ca: "never-open-fixture-ca.pem".into(),
        role: StartRole::Join,
    };
    for roster in [
        vec![],
        vec![PlayerId(0)],
        vec![PlayerId(7), PlayerId(7)],
        (1..=65).map(PlayerId).collect(),
    ] {
        assert!(matches!(
            GroupMultiplayer::host(
                address,
                IDENTITY.to_vec(),
                roster.clone(),
                MultiplayerOptions::default()
            )
            .err(),
            Some(MultiplayerError::Protocol(_))
        ));
        assert!(matches!(
            GroupMultiplayer::join(
                address,
                IDENTITY.to_vec(),
                roster.clone(),
                MultiplayerOptions::default()
            )
            .err(),
            Some(MultiplayerError::Protocol(_))
        ));
        let webtransport = GroupMultiplayer::webtransport(
            connection.clone(),
            IDENTITY.to_vec(),
            roster,
            MultiplayerOptions::default(),
        )
        .err();
        #[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
        assert!(matches!(webtransport, Some(MultiplayerError::Protocol(_))));
        #[cfg(not(all(not(target_arch = "wasm32"), feature = "webtransport")))]
        assert!(
            matches!(webtransport, Some(MultiplayerError::Io(message)) if message.contains("requires a native build"))
        );
    }
    for identity in [vec![], vec![0; MAX_IDENTITY + 1]] {
        assert_eq!(
            GroupMultiplayer::host(
                address,
                identity.clone(),
                PLAYERS.to_vec(),
                MultiplayerOptions::default()
            )
            .err(),
            Some(MultiplayerError::InvalidOptions)
        );
        assert_eq!(
            GroupMultiplayer::join(
                address,
                identity,
                PLAYERS.to_vec(),
                MultiplayerOptions::default()
            )
            .err(),
            Some(MultiplayerError::InvalidOptions)
        );
    }
    let invalid = MultiplayerOptions {
        queue_capacity: 0,
        ..MultiplayerOptions::default()
    };
    assert_eq!(
        GroupMultiplayer::join(address, IDENTITY.to_vec(), PLAYERS.to_vec(), invalid).err(),
        Some(MultiplayerError::InvalidOptions)
    );

    let mut roster: Vec<_> = (1..64).map(PlayerId).collect();
    roster.push(PlayerId(u32::MAX));
    let (mut session, prepared) = Multiplayer::prepare_session(
        IDENTITY.to_vec(),
        Some(roster.clone()),
        StartRole::Host,
        &MultiplayerOptions::default(),
    )
    .unwrap();
    assert_eq!(session.local_roster(), Some(roster.as_slice()));
    assert_eq!(prepared.as_ref().unwrap().local_roster, roster);
    let setup = match session.poll_write(0).unwrap() {
        WriteStep::Frame(frame) => frame,
        other => panic!("expected actual prepared group setup, got {other:?}"),
    };
    assert_eq!(decoded(&setup.bytes).0, 14);
    let (mut owner, channels) = front(&roster, 1);
    ready(&mut owner, &channels);
    let mut rows = members(&roster, i64::MIN, u64::MAX);
    rows[63].progress.song_ns = i64::MAX;
    owner.try_publish(rows.clone()).unwrap();
    assert_eq!(owner.local_roster(), roster.as_slice());
    assert_eq!(accepted(&owner), Some(rows.as_slice()));
    assert_eq!(
        group_message(channels.outgoing.try_recv().unwrap()),
        (rows, false)
    );
    assert!(!owner.owner.final_acknowledged);
}

#[test]
fn common_ready_and_start_gate_whole_prefix_admission_and_bad_later_members_preserve_the_previous_prefix()
 {
    let (mut owner, channels) = front(&PLAYERS, 1);
    let first = members(&PLAYERS, -1, 9_007_199_254_740_993);
    assert!(owner.try_publish(first.clone()).is_err());
    assert!(owner.try_finish(first.clone()).is_err());
    assert!(accepted(&owner).is_none());
    owner.try_ready().unwrap();
    assert!(owner.owner.ready_requested.load(Ordering::Acquire));
    assert!(owner.try_ready().is_err());
    channels
        .incoming
        .try_send(MultiplayerNotice::Session(MultiplayerEvent::Ready))
        .unwrap();
    assert_eq!(
        owner.poll(),
        vec![MultiplayerNotice::Session(MultiplayerEvent::Ready)]
    );
    assert!(
        owner.try_publish(first.clone()).is_err(),
        "readiness does not invent a committed schedule"
    );
    ready(&mut owner, &channels);
    owner.try_publish(first.clone()).unwrap();
    assert_eq!(
        group_message(channels.outgoing.try_recv().unwrap()),
        (first.clone(), false)
    );

    let mut bad_counter = first.clone();
    bad_counter[1].progress.max_combo = u64::MAX;
    let mut regressed = first.clone();
    regressed[1].progress.song_ns = -2;
    let mut reordered = first.clone();
    reordered.reverse();
    let mut duplicate = first.clone();
    duplicate[1].player = PLAYERS[0];
    let mut unknown = first.clone();
    unknown[1].player = PlayerId(91);
    for candidate in [
        bad_counter,
        regressed,
        reordered,
        duplicate,
        unknown,
        vec![],
        first[..1].to_vec(),
    ] {
        assert!(owner.try_publish(candidate.clone()).is_err());
        assert!(owner.try_finish(candidate).is_err());
        assert_eq!(accepted(&owner), Some(first.as_slice()));
        assert!(matches!(
            channels.outgoing.try_recv(),
            Err(TryRecvError::Empty)
        ));
        assert!(!owner.owner.closed && !owner.owner.local_final);
    }
    let second = members(&PLAYERS, 604_800_000_000_001, u64::MAX);
    owner.try_publish(second.clone()).unwrap();
    assert_eq!(
        group_message(channels.outgoing.try_recv().unwrap()),
        (second.clone(), false)
    );
    assert_eq!(accepted(&owner), Some(second.as_slice()));
}

#[test]
fn ordinary_saturation_fences_but_terminal_saturation_preserves_retry_and_never_claims_a_write_or_ack()
 {
    let first = members(&PLAYERS, 0, 1);
    let last = members(&PLAYERS, 1, 2);
    let (mut owner, channels) = front(&PLAYERS, 1);
    ready(&mut owner, &channels);
    owner.try_publish(first.clone()).unwrap();
    assert_eq!(
        owner.try_publish(last.clone()),
        Err(MultiplayerError::QueueFull)
    );
    assert_eq!(accepted(&owner), Some(first.as_slice()));
    assert!(owner.owner.stop_flag.load(Ordering::Acquire));
    assert_eq!(
        group_message(channels.outgoing.try_recv().unwrap()),
        (first.clone(), false)
    );
    assert_eq!(
        owner.try_finish(last.clone()),
        Err(MultiplayerError::Closed)
    );

    let (mut terminal, channels) = front(&PLAYERS, 1);
    ready(&mut terminal, &channels);
    terminal.try_publish(first.clone()).unwrap();
    assert_eq!(
        terminal.try_finish(last.clone()),
        Err(MultiplayerError::QueueFull)
    );
    assert_eq!(accepted(&terminal), Some(first.as_slice()));
    assert!(!terminal.owner.closed && !terminal.owner.local_final);
    assert!(!terminal.owner.stop_flag.load(Ordering::Acquire));
    assert_eq!(
        group_message(channels.outgoing.try_recv().unwrap()),
        (first, false)
    );
    terminal.try_finish(last.clone()).unwrap();
    assert_eq!(
        group_message(channels.outgoing.try_recv().unwrap()),
        (last.clone(), true)
    );
    assert!(terminal.owner.local_final && !terminal.owner.final_acknowledged);
    assert!(terminal.try_publish(last.clone()).is_err());
    assert!(terminal.try_finish(last.clone()).is_err());
    assert!(
        terminal.finish_delivery(last.clone()).is_err(),
        "admitted terminal content is immutable"
    );
    assert_eq!(accepted(&terminal), Some(last.as_slice()));
    terminal.request_stop();
    assert_eq!(terminal.try_publish(last), Err(MultiplayerError::Closed));
}

#[test]
fn typed_notice_queue_preserves_independent_remote_roster_and_final_prefix_before_terminal_disconnect()
 {
    let (mut owner, channels) = front(&PLAYERS, 1);
    let first = GroupPrefix {
        sequence: 0,
        final_prefix: false,
        members: members(&REMOTE, i64::MIN, 1),
    };
    let last = GroupPrefix {
        sequence: u64::MAX,
        final_prefix: true,
        members: members(&REMOTE, i64::MAX, u64::MAX),
    };
    let expected = vec![
        MultiplayerNotice::Group(GroupEvent::Roster(REMOTE.to_vec())),
        MultiplayerNotice::Session(MultiplayerEvent::Connected),
        MultiplayerNotice::Group(GroupEvent::Progress(first)),
        MultiplayerNotice::Group(GroupEvent::Progress(last.clone())),
    ];
    for notice in &expected {
        channels.incoming.try_send(notice.clone()).unwrap();
    }
    channels
        .terminal
        .try_send(MultiplayerError::IoStalled)
        .unwrap();
    let mut expected = expected;
    expected.push(MultiplayerNotice::Session(MultiplayerEvent::Disconnected(
        MultiplayerError::IoStalled,
    )));
    assert_eq!(owner.poll(), expected);
    assert_eq!(owner.local_roster(), PLAYERS.as_slice());
    assert_eq!(owner.remote_roster(), Some(REMOTE.as_slice()));
    assert_eq!(owner.remote_progress(), Some(&last));
    assert_eq!(owner.remote_final_progress(), Some(&last));
    assert!(
        owner.owner.remote.is_none() && owner.owner.remote_final.is_none(),
        "no scalar first-member alias"
    );
    assert!(!owner.is_connected() && !owner.is_ready());
    assert!(owner.poll().is_empty());
    owner.request_stop();
    owner.stop().unwrap();
    assert_eq!(owner.remote_progress(), Some(&last));
    assert_eq!(owner.remote_final_progress(), Some(&last));
}

#[test]
fn scalar_facade_keeps_scalar_events_while_wrong_mode_and_immediate_delivery_deadlines_preserve_group_state()
 {
    let rows = members(&PLAYERS, 0, 1);
    let (mut group, channels) = front(&PLAYERS, 1);
    ready(&mut group, &channels);
    assert!(group.owner.try_publish(rows[0].progress).is_err());
    assert!(group.owner.try_finish(rows[0].progress).is_err());
    assert!(accepted(&group).is_none());
    assert!(matches!(
        channels.outgoing.try_recv(),
        Err(TryRecvError::Empty)
    ));
    assert_eq!(
        group.finish_delivery(rows.clone()),
        Err(MultiplayerError::IoStalled)
    );
    assert!(!group.owner.local_final);
    group.request_stop();
    assert_eq!(
        group.finish_delivery(rows.clone()),
        Err(MultiplayerError::Closed)
    );

    let (mut scalar, channels) = channel_owner(None, 1);
    scalar.ready = true;
    scalar.start_schedule = Some(StartSchedule {
        target_ns: 2_000_000_000,
        song_target_ns: 2_100_000_000,
        uncertainty_ns: 0,
    });
    scalar.try_publish(rows[0].progress).unwrap();
    match channels.outgoing.try_recv().unwrap() {
        OutgoingMessage::Scalar(outgoing) => {
            assert_eq!(outgoing.progress, rows[0].progress);
            assert!(!outgoing.final_prefix);
        }
        OutgoingMessage::Group { .. } => panic!("scalar API changed protocol mode"),
    }
    channels
        .incoming
        .try_send(MultiplayerNotice::Session(MultiplayerEvent::FinalProgress(
            rows[1].progress,
        )))
        .unwrap();
    assert_eq!(
        scalar.poll(),
        vec![MultiplayerEvent::FinalProgress(rows[1].progress)]
    );
    assert_eq!(scalar.remote_progress(), Some(rows[1].progress));
    assert_eq!(scalar.remote_final_progress(), Some(rows[1].progress));
    assert!(scalar.group.is_none());
}

fn decoded(bytes: &[u8]) -> (u8, Vec<u8>) {
    let mut decoder = Frames::new();
    for chunk in bytes.chunks(3) {
        let mut offset = 0;
        while offset < chunk.len() {
            let used = decoder.push(&chunk[offset..]).unwrap();
            assert!(used > 0);
            offset += used;
        }
    }
    decoder.take().unwrap().unwrap()
}

fn receive_frame(peer: &mut Session, frame: &OutboundFrame, now: i64) {
    let (tag, payload) = decoded(&frame.bytes);
    peer.receive(tag, &payload, now).unwrap();
}

fn started_pair() -> (Session, Session, i64) {
    let policy = StartPolicy {
        lead_ns: 10_000,
        min_remaining_ns: 100,
        max_age_ns: 100_000,
        max_uncertainty_ns: 100,
        max_release_lateness_ns: 25,
    };
    let mut host = Session::new_group(
        IDENTITY.to_vec(),
        PLAYERS.to_vec(),
        StartRole::Host,
        policy,
        100,
    )
    .unwrap();
    let mut join = Session::new_group(
        IDENTITY.to_vec(),
        REMOTE.to_vec(),
        StartRole::Join,
        policy,
        200,
    )
    .unwrap();
    host.request_ready().unwrap();
    join.request_ready().unwrap();
    let mut now = 0;
    for _ in 0..128 {
        now += 10;
        let left = host.poll_write(now).unwrap();
        let right = join.poll_write(now).unwrap();
        if let WriteStep::Frame(frame) = &left {
            host.written(frame.id, now + 1).unwrap();
        }
        if let WriteStep::Frame(frame) = &right {
            join.written(frame.id, now + 1).unwrap();
        }
        if let WriteStep::Frame(frame) = &left {
            receive_frame(&mut join, frame, now + 2);
        }
        if let WriteStep::Frame(frame) = &right {
            receive_frame(&mut host, frame, now + 2);
        }
        now += 2;
        if host.start_committed() && join.start_committed() {
            return (host, join, now);
        }
    }
    panic!("bounded common Session setup/readiness/probe/start exchange did not commit")
}

#[test]
fn actual_worker_dispatch_waits_for_session_slot_and_complete_final_write_before_real_peer_ack_notice()
 {
    let mut unprepared = Session::new_group(
        IDENTITY.to_vec(),
        PLAYERS.to_vec(),
        StartRole::Host,
        StartPolicy::default(),
        0,
    )
    .unwrap();
    assert!(
        OutgoingMessage::Group {
            members: members(&PLAYERS, 0, 1),
            final_prefix: false
        }
        .send(&mut unprepared, 0)
        .is_err()
    );
    let (mut session, mut peer, mut now) = started_pair();
    let (mut front, channels) = front(&PLAYERS, 1);
    forward_session_events(&mut session, &channels.incoming).unwrap();
    let notices = front.poll();
    assert!(
        notices.contains(&MultiplayerNotice::Group(GroupEvent::Roster(
            REMOTE.to_vec()
        )))
    );
    assert!(notices.contains(&MultiplayerNotice::Session(MultiplayerEvent::Ready)));
    assert!(front.is_ready() && front.start_schedule().is_some());
    assert_eq!(front.remote_roster(), Some(REMOTE.as_slice()));

    let rows = members(&PLAYERS, 604_800_000_000_001, 9_007_199_254_740_993);
    front.try_finish(rows.clone()).unwrap();
    assert!(!front.owner.final_acknowledged);
    now += 1;
    assert_eq!(session.poll_write(now).unwrap(), WriteStep::ApplicationSlot);
    let sent = channels
        .outgoing
        .try_recv()
        .unwrap()
        .send(&mut session, now)
        .unwrap();
    assert_eq!(decoded(&sent.bytes).0, 13);
    assert_eq!(session.local_group_progress().unwrap().members, rows);
    assert_eq!(session.poll_write(now + 1).unwrap(), WriteStep::Waiting);
    let mut prefix = Frames::new();
    let split = sent.bytes.len() / 2;
    let mut offset = 0;
    while offset < split {
        let used = prefix.push(&sent.bytes[offset..split]).unwrap();
        assert!(used > 0);
        offset += used;
    }
    assert!(prefix.take().unwrap().is_none());
    assert!(peer.remote_group_progress().is_none());
    forward_session_events(&mut session, &channels.incoming).unwrap();
    assert!(front.poll().is_empty());
    assert!(
        !front.owner.final_acknowledged,
        "neither queue acceptance nor a partial frame grants acknowledgement"
    );

    now += 2;
    session.written(sent.id, now).unwrap();
    receive_frame(&mut peer, &sent, now);
    let ack = match peer.poll_write(now + 1).unwrap() {
        WriteStep::Frame(frame) => frame,
        other => panic!("expected actual final ACK frame, got {other:?}"),
    };
    assert_eq!(decoded(&ack.bytes), (4, 0u64.to_le_bytes().to_vec()));
    assert!(!front.owner.final_acknowledged);
    peer.written(ack.id, now + 2).unwrap();
    receive_frame(&mut session, &ack, now + 3);
    forward_session_events(&mut session, &channels.incoming).unwrap();
    assert_eq!(
        front.poll(),
        vec![MultiplayerNotice::Session(
            MultiplayerEvent::FinalAcknowledged
        )]
    );
    assert!(front.owner.final_acknowledged);
    channels
        .terminal
        .try_send(MultiplayerError::Closed)
        .unwrap();
    assert_eq!(
        front.owner.wait_for_notice_delivery(
            OutgoingMessage::Group {
                members: rows.clone(),
                final_prefix: true
            },
            Instant::now(),
        ),
        Ok(()),
        "the shared delivery loop uses the genuine ACK even when EOF follows it"
    );
    assert!(front.owner.closed);
    assert_eq!(accepted(&front), Some(rows.as_slice()));
    assert!(
        front.owner.final_acknowledged,
        "ordinary transport EOF cannot erase the actual application receipt"
    );
}
