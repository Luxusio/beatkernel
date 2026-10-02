//! Authored portable session fixtures; execution is deferred.
use crate::multiplayer_protocol::{
    FrameDecoder, MultiplayerError, MultiplayerEvent, OutboundFrame, Progress, Session, WriteStep,
};
use crate::multiplayer_start::{StartPolicy, StartRole};

const IDENTITY: &[u8] = b"same canonical replay identity\0\xff";

fn policy() -> StartPolicy {
    StartPolicy {
        lead_ns: 10_000,
        min_remaining_ns: 100,
        max_age_ns: 100_000,
        max_uncertainty_ns: 100,
        max_release_lateness_ns: 25,
    }
}

fn session(role: StartRole) -> Session {
    Session::new(IDENTITY.to_vec(), role, policy(), 0).unwrap()
}

fn frame(step: WriteStep) -> OutboundFrame {
    match step {
        WriteStep::Frame(frame) => frame,
        other => panic!("expected an admitted frame, got {other:?}"),
    }
}

fn decode(wire: &[u8]) -> (u8, Vec<u8>) {
    let mut decoder = FrameDecoder::new();
    // Exercise the same decoder a transport uses, including split headers.
    for chunk in wire.chunks(3) {
        let mut offset = 0;
        while offset < chunk.len() {
            let count = decoder.push(&chunk[offset..]).unwrap();
            assert!(count > 0);
            offset += count;
        }
    }
    decoder.take().unwrap().unwrap()
}

fn receive(owner: &mut Session, frame: &OutboundFrame, now: i64) {
    let (tag, payload) = decode(&frame.bytes);
    owner.receive(tag, &payload, now).unwrap();
}

fn drain(owner: &mut Session) -> Vec<MultiplayerEvent> {
    std::iter::from_fn(|| owner.poll_event()).collect()
}

fn progress(song_ns: i64, hits: u64, misses: u64, combo: u64) -> Progress {
    Progress {
        song_ns,
        hits,
        misses,
        combo,
        max_combo: hits,
    }
}

struct Pair {
    host: Session,
    join: Session,
    now: i64,
    events: [Vec<MultiplayerEvent>; 2],
    tags: [Vec<u8>; 2],
    last_ids: [u64; 2],
}

impl Pair {
    fn new() -> Self {
        let mut host = Session::new(IDENTITY.to_vec(), StartRole::Host, policy(), 100).unwrap();
        let mut join = Session::new(IDENTITY.to_vec(), StartRole::Join, policy(), 200).unwrap();
        host.request_ready().unwrap();
        join.request_ready().unwrap();
        Self {
            host,
            join,
            now: 0,
            events: [vec![], vec![]],
            tags: [vec![], vec![]],
            last_ids: [0, 0],
        }
    }

    fn cycle(&mut self) {
        self.now += 10;
        let host = self.host.poll_write(self.now).unwrap();
        let join = self.join.poll_write(self.now).unwrap();
        let mut pending = [None, None];
        for (index, step) in [host, join].into_iter().enumerate() {
            if let WriteStep::Frame(frame) = step {
                assert!(frame.id > self.last_ids[index]);
                self.last_ids[index] = frame.id;
                self.tags[index].push(decode(&frame.bytes).0);
                pending[index] = Some(frame);
            }
        }
        // Both complete writes happen before either frame is delivered.
        if let Some(frame) = &pending[0] {
            self.host.written(frame.id, self.now + 1).unwrap();
        }
        if let Some(frame) = &pending[1] {
            self.join.written(frame.id, self.now + 1).unwrap();
        }
        if let Some(frame) = &pending[0] {
            receive(&mut self.join, frame, self.now + 2);
        }
        if let Some(frame) = &pending[1] {
            receive(&mut self.host, frame, self.now + 2);
        }
        self.now += 2;
        self.events[0].extend(drain(&mut self.host));
        self.events[1].extend(drain(&mut self.join));
    }

    fn started() -> Self {
        let mut pair = Self::new();
        for _ in 0..128 {
            pair.cycle();
            if pair.host.start_committed() && pair.join.start_committed() {
                return pair;
            }
        }
        panic!("finite caller-driven exchange did not commit both starts")
    }
}

fn assert_fenced(owner: &mut Session, error: MultiplayerError, now: i64) {
    assert_eq!(owner.request_ready(), Err(error.clone()));
    assert_eq!(owner.receive(1, IDENTITY, now), Err(error.clone()));
    assert_eq!(owner.poll_write(now), Err(error.clone()));
    assert_eq!(owner.written(1, now), Err(error.clone()));
    assert_eq!(
        owner.send_progress(progress(0, 0, 0, 0), false, now),
        Err(error)
    );
}

#[test]
fn exact_identity_setup_rejects_incompatible_order_and_retains_prior_events() {
    assert!(Session::new(vec![], StartRole::Host, policy(), 0).is_err());
    assert!(Session::new(vec![0; 65_537], StartRole::Host, policy(), 0).is_err());
    assert!(Session::new(IDENTITY.to_vec(), StartRole::Host, policy(), -1).is_err());
    let mut owner = session(StartRole::Host);
    assert!(!owner.setup_complete());
    assert!(owner.preparation_pending());
    let setup = frame(owner.poll_write(0).unwrap());
    assert_eq!(setup.id, 1);
    let mut literal = vec![39, 0, 0, 0, b'B', b'K', b'M', b'P', 6, 0, 1];
    literal.extend_from_slice(IDENTITY);
    assert_eq!(setup.bytes, literal);
    owner.receive(1, IDENTITY, 0).unwrap();
    assert!(owner.setup_complete());
    assert!(owner.preparation_pending());
    let duplicate = owner.receive(1, IDENTITY, 0).unwrap_err();
    assert_eq!(drain(&mut owner), vec![MultiplayerEvent::Connected]);
    assert_fenced(&mut owner, duplicate, 0);

    for (tag, payload) in [(1, b"different".as_slice()), (5, &[]), (255, &[])] {
        let mut owner = session(StartRole::Join);
        let error = owner.receive(tag, payload, 0).unwrap_err();
        if tag == 1 {
            assert_eq!(error, MultiplayerError::IncompatibleSetup);
        }
        assert!(!owner.setup_complete());
        assert!(owner.poll_event().is_none());
        assert_fenced(&mut owner, error, 0);
    }
}

#[test]
fn readiness_waits_for_full_frame_receipt_and_backpressure_keeps_exact_owner_bytes() {
    let mut host = session(StartRole::Host);
    let mut join = session(StartRole::Join);
    host.request_ready().unwrap();
    join.request_ready().unwrap();
    let host_setup = frame(host.poll_write(0).unwrap());
    let join_setup = frame(join.poll_write(0).unwrap());
    receive(&mut host, &join_setup, 1);
    receive(&mut join, &host_setup, 1);
    let held = host_setup.bytes.clone();
    for now in 1..=3 {
        assert_eq!(host.poll_write(now).unwrap(), WriteStep::Waiting);
        assert_eq!(host_setup.bytes, held);
    }
    assert_eq!(drain(&mut host), vec![MultiplayerEvent::Connected]);
    assert_eq!(drain(&mut join), vec![MultiplayerEvent::Connected]);
    host.written(host_setup.id, 3).unwrap();
    join.written(join_setup.id, 3).unwrap();
    let host_ready = frame(host.poll_write(4).unwrap());
    let join_ready = frame(join.poll_write(4).unwrap());
    assert_eq!(decode(&host_ready.bytes), (5, vec![]));
    assert!(host_ready.id > host_setup.id);
    receive(&mut host, &join_ready, 5);
    receive(&mut join, &host_ready, 5);
    assert!(drain(&mut host).is_empty());
    assert!(drain(&mut join).is_empty());
    assert_eq!(host.poll_write(6).unwrap(), WriteStep::Waiting);
    host.written(host_ready.id, 6).unwrap();
    assert_eq!(drain(&mut host), vec![MultiplayerEvent::Ready]);
    assert!(drain(&mut join).is_empty());
    join.written(join_ready.id, 6).unwrap();
    assert_eq!(drain(&mut join), vec![MultiplayerEvent::Ready]);
    assert!(host.preparation_pending() && join.preparation_pending());
    assert!(!host.start_committed());
}

#[test]
fn eight_real_probe_exchanges_produce_bilateral_start_and_application_slots() {
    let mut pair = Pair::started();
    for index in 0..2 {
        assert_eq!(pair.tags[index].iter().filter(|&&tag| tag == 6).count(), 8);
        assert_eq!(pair.tags[index].iter().filter(|&&tag| tag == 7).count(), 8);
        assert_eq!(pair.events[index].len(), 4);
        assert_eq!(pair.events[index][0], MultiplayerEvent::Connected);
        assert_eq!(pair.events[index][1], MultiplayerEvent::Ready);
        let MultiplayerEvent::ClockEstimated(estimate) = pair.events[index][2] else {
            panic!("missing real probe estimate")
        };
        assert_eq!(
            (
                estimate.lower_ns(),
                estimate.upper_ns(),
                estimate.round_trip_ns()
            ),
            (-2, 2, 4)
        );
    }
    let MultiplayerEvent::StartScheduled(host) = pair.events[0][3] else {
        panic!("missing host schedule")
    };
    let MultiplayerEvent::StartScheduled(join) = pair.events[1][3] else {
        panic!("missing join schedule")
    };
    assert_eq!(host.song_target_ns, join.song_target_ns);
    assert_eq!(host.target_ns + 100, host.song_target_ns);
    assert_eq!(join.target_ns + 200, join.song_target_ns);
    assert!(host.target_ns > pair.now && join.target_ns > pair.now);
    assert!(!pair.host.preparation_pending());
    assert!(!pair.join.preparation_pending());
    assert_eq!(
        pair.host.poll_write(pair.now).unwrap(),
        WriteStep::ApplicationSlot
    );
    assert_eq!(
        pair.join.poll_write(pair.now).unwrap(),
        WriteStep::ApplicationSlot
    );
    assert!(pair.host.poll_event().is_none());
    assert!(pair.join.poll_event().is_none());
}

#[test]
fn early_progress_and_invalid_completion_receipts_fence_without_false_readiness() {
    let mut early = session(StartRole::Host);
    let error = early
        .send_progress(progress(-1, 0, 0, 0), false, 0)
        .unwrap_err();
    assert!(!early.start_committed());
    assert_fenced(&mut early, error, 0);
    for wrong_id in [0, 2, u64::MAX] {
        let mut owner = session(StartRole::Host);
        let setup = frame(owner.poll_write(0).unwrap());
        assert_eq!(setup.id, 1);
        let error = owner.written(wrong_id, 1).unwrap_err();
        assert!(!owner.setup_complete());
        assert!(owner.poll_event().is_none());
        assert_fenced(&mut owner, error, 1);
    }
    let mut duplicate = session(StartRole::Host);
    let setup = frame(duplicate.poll_write(0).unwrap());
    duplicate.written(setup.id, 0).unwrap();
    let error = duplicate.written(setup.id, 0).unwrap_err();
    assert_fenced(&mut duplicate, error, 0);

    let mut prestart = session(StartRole::Join);
    prestart.receive(1, IDENTITY, 0).unwrap();
    let mut payload = vec![0; 48];
    payload[8..16].copy_from_slice(&(-100_i64).to_le_bytes());
    let error = prestart.receive(2, &payload, 0).unwrap_err();
    assert_eq!(drain(&mut prestart), vec![MultiplayerEvent::Connected]);
    assert_fenced(&mut prestart, error, 0);
}

#[test]
fn final_ack_events_require_exact_local_and_peer_ack_write_receipts() {
    let mut pair = Pair::started();
    let now = pair.now;
    assert_eq!(
        pair.host.poll_write(now).unwrap(),
        WriteStep::ApplicationSlot
    );
    let ordinary = progress(-100, 1, 0, 1);
    let wire = pair.host.send_progress(ordinary, false, now).unwrap();
    assert_eq!(pair.host.poll_write(now).unwrap(), WriteStep::Waiting);
    pair.host.written(wire.id, now).unwrap();
    receive(&mut pair.join, &wire, now);
    assert_eq!(
        drain(&mut pair.join),
        vec![MultiplayerEvent::Progress(ordinary)]
    );

    assert_eq!(
        pair.host.poll_write(now).unwrap(),
        WriteStep::ApplicationSlot
    );
    assert_eq!(
        pair.join.poll_write(now).unwrap(),
        WriteStep::ApplicationSlot
    );
    let host_final = progress(50, 2, 0, 2);
    let join_final = progress(51, 0, 1, 0);
    let host_wire = pair.host.send_progress(host_final, true, now).unwrap();
    let join_wire = pair.join.send_progress(join_final, true, now).unwrap();
    receive(&mut pair.join, &host_wire, now);
    receive(&mut pair.host, &join_wire, now);
    assert_eq!(
        drain(&mut pair.host),
        vec![MultiplayerEvent::FinalProgress(join_final)]
    );
    assert_eq!(
        drain(&mut pair.join),
        vec![MultiplayerEvent::FinalProgress(host_final)]
    );
    assert_eq!(pair.host.poll_write(now).unwrap(), WriteStep::Waiting);
    pair.host.written(host_wire.id, now).unwrap();
    pair.join.written(join_wire.id, now).unwrap();
    let host_ack = frame(pair.host.poll_write(now).unwrap());
    let join_ack = frame(pair.join.poll_write(now).unwrap());
    assert_eq!(decode(&host_ack.bytes).0, 4);
    assert_eq!(decode(&join_ack.bytes).0, 4);
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
    assert!(pair.host.poll_event().is_none() && pair.join.poll_event().is_none());
}

#[test]
fn received_work_invalidates_application_grant_and_negative_time_stays_fatal() {
    let mut pair = Pair::started();
    assert_eq!(
        pair.host.poll_write(pair.now).unwrap(),
        WriteStep::ApplicationSlot
    );
    assert_eq!(
        pair.join.poll_write(pair.now).unwrap(),
        WriteStep::ApplicationSlot
    );
    let peer = pair
        .join
        .send_progress(progress(0, 1, 0, 1), false, pair.now)
        .unwrap();
    pair.join.written(peer.id, pair.now).unwrap();
    receive(&mut pair.host, &peer, pair.now);
    let error = pair
        .host
        .send_progress(progress(0, 0, 0, 0), false, pair.now)
        .unwrap_err();
    assert_eq!(
        drain(&mut pair.host),
        vec![MultiplayerEvent::Progress(progress(0, 1, 0, 1))]
    );
    assert_fenced(&mut pair.host, error, pair.now);

    let mut negative = session(StartRole::Host);
    let error = negative.poll_write(-1).unwrap_err();
    assert!(negative.poll_event().is_none());
    assert_fenced(&mut negative, error, 0);
    let mut backwards = session(StartRole::Host);
    let setup = frame(backwards.poll_write(10).unwrap());
    let error = backwards.written(setup.id, 9).unwrap_err();
    assert_fenced(&mut backwards, error, 10);
}

#[test]
fn event_capacity_failure_retains_exact_accepted_prefix_instead_of_dropping() {
    let mut pair = Pair::started();
    let mut expected = Vec::new();
    for index in 1..=9 {
        assert_eq!(
            pair.host.poll_write(pair.now).unwrap(),
            WriteStep::ApplicationSlot
        );
        let progress = progress(index, index as u64, 0, index as u64);
        let wire = pair.host.send_progress(progress, false, pair.now).unwrap();
        pair.host.written(wire.id, pair.now).unwrap();
        let (tag, payload) = decode(&wire.bytes);
        let received = pair.join.receive(tag, &payload, pair.now);
        if index <= 8 {
            received.unwrap();
            expected.push(MultiplayerEvent::Progress(progress));
        } else {
            assert_eq!(received, Err(MultiplayerError::QueueFull));
        }
    }
    assert_eq!(drain(&mut pair.join), expected);
    assert_fenced(&mut pair.join, MultiplayerError::QueueFull, pair.now);
    assert!(pair.join.poll_event().is_none());
}
