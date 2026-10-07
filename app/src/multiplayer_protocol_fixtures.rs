//! Deferred portable fixtures; no sockets or platform clocks are needed.
use crate::multiplayer_clock::OffsetEstimate;
use crate::multiplayer_protocol::*;
use crate::multiplayer_start::{StartAgreement, StartMessage, StartPolicy, StartRole};

// Literal wire vectors deliberately do not use the production encoder.
const READY: &[u8] = &[7, 0, 0, 0, b'B', b'K', b'M', b'P', 6, 0, 5];
const EXTREME_PROGRESS: &[u8] = &[
    55, 0, 0, 0, b'B', b'K', b'M', b'P', 6, 0, 2, 255, 255, 255, 255, 255, 255, 255,
    255, // sequence: u64::MAX
    0, 0, 0, 0, 0, 0, 0, 128, // song: i64::MIN
    255, 255, 255, 255, 255, 255, 255, 255, // hits
    0, 0, 0, 0, 0, 0, 0, 0, // misses
    255, 255, 255, 255, 255, 255, 255, 255, // combo
    255, 255, 255, 255, 255, 255, 255, 255, // max combo
];
const START: &[u8] = &[
    15, 0, 0, 0, b'B', b'K', b'M', b'P', 6, 0, 9, 8, 7, 6, 5, 4, 3, 2, 1,
];

fn decode(wire: &[u8]) -> (u8, Vec<u8>) {
    let mut decoder = FrameDecoder::new();
    let mut consumed = 0;
    while consumed < wire.len() {
        let count = decoder.push(&wire[consumed..]).unwrap();
        assert!(count > 0, "fixture contains exactly one frame");
        consumed += count;
    }
    let result = decoder.take().unwrap().unwrap();
    assert_eq!(decoder.needed().unwrap(), 4);
    result
}

fn progress(song_ns: i64, hits: u64, misses: u64, combo: u64, max_combo: u64) -> Progress {
    Progress {
        song_ns,
        hits,
        misses,
        combo,
        max_combo,
    }
}

fn outgoing(progress: Progress, final_prefix: bool) -> Outgoing {
    Outgoing {
        progress,
        final_prefix,
    }
}

fn receive(protocol: &mut Protocol, wire: &[u8]) -> Option<MultiplayerEvent> {
    let (tag, payload) = decode(wire);
    protocol.receive(tag, &payload).unwrap()
}

fn ready_protocol() -> Protocol {
    let mut protocol = Protocol::default();
    assert_eq!(protocol.next_ready(true).unwrap(), READY);
    protocol.written(5);
    assert_eq!(receive(&mut protocol, READY), None);
    assert_eq!(protocol.readiness(), Some(MultiplayerEvent::Ready));
    protocol
}

#[test]
fn literal_v6_frames_preserve_full_width_values_and_native_type_identity() {
    assert_eq!(encode_frame(5, &[]).unwrap(), READY);
    let extreme = progress(i64::MIN, u64::MAX, 0, u64::MAX, u64::MAX);
    assert_eq!(progress_frame(u64::MAX, extreme), EXTREME_PROGRESS);
    assert_eq!(
        encode_frame(2, &EXTREME_PROGRESS[11..]).unwrap(),
        EXTREME_PROGRESS
    );
    let (tag, payload) = decode(EXTREME_PROGRESS);
    assert_eq!(tag, 2);
    assert_eq!(parse_progress(&payload, u64::MAX, None).unwrap(), extreme);
    assert!(parse_progress(&payload, u64::MAX - 1, None).is_err());

    let message = StartMessage::Propose(0x0102_0304_0506_0708);
    assert_eq!(start_frame(message), START);
    let (tag, payload) = decode(START);
    assert_eq!(parse_start_frame(tag, &payload).unwrap(), message);
    for value in [i64::MIN, -1, i64::MAX] {
        let bytes = value.to_le_bytes();
        for (tag, message) in [
            (8, StartMessage::ClockReady(value)),
            (9, StartMessage::Propose(value)),
            (10, StartMessage::Accept(value)),
            (11, StartMessage::Commit(value)),
        ] {
            assert_eq!(parse_start_frame(tag, &bytes).unwrap(), message);
            assert_eq!(&start_frame(message)[11..], &bytes);
        }
    }

    // Assignments compile only when public native paths retain identical types.
    let native_progress: crate::multiplayer::Progress = extreme;
    let common_progress: Progress = native_progress;
    assert_eq!(common_progress, extreme);
    let native_event: crate::multiplayer::MultiplayerEvent = MultiplayerEvent::Progress(extreme);
    let common_event: MultiplayerEvent = native_event;
    assert_eq!(common_event, MultiplayerEvent::Progress(extreme));
    let native_error: crate::multiplayer::MultiplayerError = MultiplayerError::QueueFull;
    let common_error: MultiplayerError = native_error;
    assert_eq!(common_error, MultiplayerError::QueueFull);
}

#[test]
fn decoder_accepts_fragmented_prefixes_and_leaves_coalesced_suffix_with_caller() {
    let mut stream = READY.to_vec();
    stream.extend_from_slice(EXTREME_PROGRESS);
    stream.extend_from_slice(START);
    for chunk_size in [1, 2, 3, 4, 5, 10, 11, 17, 64, 256] {
        let mut decoder = FrameDecoder::new();
        let mut received = Vec::new();
        for chunk in stream.chunks(chunk_size) {
            let mut used = 0;
            while used < chunk.len() {
                let needed = decoder.needed().unwrap();
                assert_eq!(decoder.push(&[]).unwrap(), 0);
                let copied = decoder.push(&chunk[used..]).unwrap();
                assert_eq!(copied, needed.min(chunk.len() - used));
                used += copied;
                if decoder.needed().unwrap() == 0 {
                    // A transport must take the held frame before retrying the suffix.
                    assert_eq!(decoder.push(&chunk[used..]).unwrap(), 0);
                    assert_eq!(decoder.push(READY).unwrap(), 0);
                    received.push(decoder.take().unwrap().unwrap());
                } else {
                    assert!(decoder.take().unwrap().is_none());
                }
            }
        }
        assert_eq!(
            received,
            vec![
                (5, vec![]),
                (2, EXTREME_PROGRESS[11..].to_vec()),
                (9, START[11..].to_vec()),
            ]
        );
        assert_eq!(decoder.needed().unwrap(), 4);
        assert!(decoder.take().unwrap().is_none());
    }
}

#[test]
fn framing_caps_and_bad_headers_reject_before_body_admission() {
    let payload = vec![0xa5; 65_536];
    let wire = encode_frame(1, &payload).unwrap();
    assert_eq!(&wire[..11], &[7, 0, 1, 0, b'B', b'K', b'M', b'P', 6, 0, 1]);
    assert_eq!(decode(&wire), (1, payload));
    assert!(encode_frame(1, &vec![0; 65_537]).is_err());

    for length in [0_u32, 6, 65_544, u32::MAX] {
        let header = length.to_le_bytes();
        let mut coalesced = header.to_vec();
        coalesced.extend_from_slice(READY);
        let mut decoder = FrameDecoder::new();
        assert_eq!(decoder.push(&coalesced).unwrap(), 4);
        assert_eq!(decoder.bytes, header);
        assert!(decoder.needed().is_err());
        assert!(decoder.push(&coalesced[4..]).is_err());
        assert_eq!(decoder.bytes, header);
        assert!(decoder.take().is_err());
    }
    for offset in [4, 8, 9] {
        let mut wire = READY.to_vec();
        wire[offset] ^= 1;
        let mut decoder = FrameDecoder::new();
        assert_eq!(decoder.push(&wire).unwrap(), 4);
        assert_eq!(decoder.push(&wire[4..]).unwrap(), wire.len() - 4);
        assert!(decoder.take().is_err());
    }
    // Legacy crate-private access can produce an overshoot; checked admission must not panic.
    let mut overshot = FrameDecoder::new();
    overshot.bytes.extend_from_slice(READY);
    overshot.bytes.push(0);
    assert!(overshot.needed().is_err());
    assert!(overshot.push(&[1]).is_err());
    assert!(overshot.take().is_err());

    let (tag, payload) = decode(&encode_frame(255, &[]).unwrap());
    assert_eq!(tag, 255); // Framing deliberately leaves session semantics to Protocol.
    let mut protocol = ready_protocol();
    assert!(protocol.receive(tag, &payload).is_err());
    assert!(protocol.ready());
    assert!(protocol.receive(2, &EXTREME_PROGRESS[11..]).is_err());
}

#[test]
fn readiness_requires_remote_receipt_and_complete_local_write() {
    let mut local = Protocol::default();
    let mut peer = Protocol::default();
    let initial = outgoing(progress(-1, 0, 0, 0, 0), false);
    assert!(local.next_ready(false).is_none());
    assert!(local.outgoing(initial).is_err());
    assert!(local.receive(2, &EXTREME_PROGRESS[11..]).is_err());

    let local_wire = local.next_ready(true).unwrap();
    let peer_wire = peer.next_ready(true).unwrap();
    assert_eq!(local_wire, READY);
    assert_eq!(receive(&mut local, &peer_wire), None);
    assert_eq!(receive(&mut peer, &local_wire), None);
    assert!(!local.ready());
    assert!(!peer.ready());
    assert!(local.readiness().is_none());
    assert!(local.next_ready(true).is_none());
    assert!(local.outgoing(initial).is_err());
    assert!(local.receive(5, &[]).is_err());
    local.written(2); // Completing an unrelated frame is not readiness proof.
    assert!(local.readiness().is_none());
    local.written(5);
    assert_eq!(local.readiness(), Some(MultiplayerEvent::Ready));
    assert!(local.readiness().is_none());
    assert!(!peer.ready());
    peer.written(5);
    assert_eq!(peer.readiness(), Some(MultiplayerEvent::Ready));
    assert_eq!(
        receive(&mut peer, &local.outgoing(initial).unwrap()),
        Some(MultiplayerEvent::Progress(initial.progress))
    );

    let mut written_first = Protocol::default();
    written_first.next_ready(true).unwrap();
    written_first.written(5);
    assert!(!written_first.ready());
    assert!(written_first.receive(5, &[0]).is_err());
    assert_eq!(receive(&mut written_first, READY), None);
    assert_eq!(written_first.readiness(), Some(MultiplayerEvent::Ready));
}

#[test]
fn progress_rejections_preserve_sequences_counts_and_valid_next_transition() {
    let mut sender = ready_protocol();
    let mut receiver = ready_protocol();
    let first = progress(-100, 2, 1, 1, 2);
    let first_wire = sender.outgoing(outgoing(first, false)).unwrap();
    assert_eq!(
        receive(&mut receiver, &first_wire),
        Some(MultiplayerEvent::Progress(first))
    );
    let invalid = [
        progress(-101, 2, 1, 1, 2),        // song regression
        progress(-100, 1, 1, 1, 1),        // cumulative hits regression
        progress(-100, 2, 0, 1, 2),        // cumulative misses regression
        progress(-100, 2, 1, 1, 1),        // max-combo regression
        progress(-100, 2, 1, 2, 2),        // combo grows without a hit
        progress(-100, 3, 1, 1, 2),        // no miss, but combo did not advance
        progress(-100, 3, 2, 3, 3),        // impossible combo growth despite a miss
        progress(-100, u64::MAX, 1, 0, 2), // total count overflow
        progress(-100, 2, 2, 3, 2),        // combo exceeds max
        progress(-100, 2, 2, 0, 3),        // max exceeds hits
    ];
    for rejected in invalid {
        assert!(sender.outgoing(outgoing(rejected, true)).is_err());
        let (_, payload) = decode(&progress_frame(1, rejected));
        assert!(receiver.receive(3, &payload).is_err());
        assert_eq!(
            (sender.tx_sequence, sender.local, sender.local_final),
            (1, Some(first), None)
        );
        assert_eq!(
            (
                receiver.rx_sequence,
                receiver.remote,
                receiver.remote_final,
                receiver.pending_ack
            ),
            (1, Some(first), false, None)
        );
    }
    let next = progress(0, 3, 1, 2, 2);
    let (_, wrong_sequence) = decode(&progress_frame(2, next));
    assert!(receiver.receive(2, &wrong_sequence).is_err());
    let (_, correct) = decode(&progress_frame(1, next));
    assert!(receiver.receive(2, &correct[..47]).is_err());
    assert_eq!(
        receive(
            &mut receiver,
            &sender.outgoing(outgoing(next, false)).unwrap()
        ),
        Some(MultiplayerEvent::Progress(next))
    );
    let reset = progress(1, 3, 2, 0, 2);
    assert_eq!(
        receive(
            &mut receiver,
            &sender.outgoing(outgoing(reset, false)).unwrap()
        ),
        Some(MultiplayerEvent::Progress(reset))
    );

    // Exhausted sequence space must not commit otherwise valid final progress.
    sender.tx_sequence = u64::MAX;
    receiver.rx_sequence = u64::MAX;
    assert!(sender.outgoing(outgoing(reset, true)).is_err());
    let (_, exhausted) = decode(&prefix_frame(3, u64::MAX, reset));
    assert!(receiver.receive(3, &exhausted).is_err());
    assert_eq!((sender.local, sender.local_final), (Some(reset), None));
    assert_eq!(
        (receiver.remote, receiver.remote_final, receiver.pending_ack),
        (Some(reset), false, None)
    );
}

#[test]
fn final_completion_waits_for_full_write_application_ack_and_peer_ack_write() {
    let mut left = ready_protocol();
    let mut right = ready_protocol();
    let left_final = progress(5, 1, 0, 1, 1);
    let right_final = progress(8, 0, 1, 0, 0);
    let left_wire = left.outgoing(outgoing(left_final, true)).unwrap();
    let right_wire = right.outgoing(outgoing(right_final, true)).unwrap();
    assert!(left.acknowledgement().is_none());
    assert!(right.acknowledgement().is_none());
    assert_eq!(
        receive(&mut right, &left_wire),
        Some(MultiplayerEvent::FinalProgress(left_final))
    );
    assert_eq!(
        receive(&mut left, &right_wire),
        Some(MultiplayerEvent::FinalProgress(right_final))
    );

    let ack_from_right = right.next_ack().unwrap();
    assert_eq!(
        ack_from_right,
        [
            15, 0, 0, 0, b'B', b'K', b'M', b'P', 6, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0
        ]
    );
    let (tag, payload) = decode(&ack_from_right);
    assert!(left.receive(tag, &payload).is_err()); // Merely queued final is insufficient.
    assert!(!left.local_ack_received);
    left.written(3);
    right.written(3);
    assert!(left.receive(4, &[0; 7]).is_err());
    assert!(left.receive(4, &1_u64.to_le_bytes()).is_err());
    assert_eq!(left.receive(tag, &payload).unwrap(), None);
    assert!(left.acknowledgement().is_none()); // Still owes the peer an application ACK.
    assert!(left.receive(tag, &payload).is_err());

    let ack_from_left = left.next_ack().unwrap();
    assert!(left.next_ack().is_none());
    assert!(left.acknowledgement().is_none()); // ACK admission is not a completed write.
    assert_eq!(receive(&mut right, &ack_from_left), None);
    assert!(right.acknowledgement().is_none());
    left.written(2);
    assert!(left.acknowledgement().is_none());
    left.written(4);
    assert_eq!(
        left.acknowledgement(),
        Some(MultiplayerEvent::FinalAcknowledged)
    );
    assert!(right.acknowledgement().is_none());
    right.written(4);
    assert_eq!(
        right.acknowledgement(),
        Some(MultiplayerEvent::FinalAcknowledged)
    );
    assert!(left.acknowledgement().is_none());
    assert!(right.acknowledgement().is_none());
    assert!(left.outgoing(outgoing(left_final, false)).is_err());
    let (_, after_final) = decode(&progress_frame(1, right_final));
    assert!(left.receive(2, &after_final).is_err());
}

fn symmetric_estimates() -> (OffsetEstimate, OffsetEstimate) {
    let mut left = ClockProbes::default();
    let mut right = ClockProbes::default();
    for round in 0..8 {
        let now = 1_000 + round * 1_000;
        let (_, left_ping) = decode(&left.next_ping(now).unwrap().unwrap());
        let (_, right_ping) = decode(&right.next_ping(now + 100).unwrap().unwrap());
        right.receive_ping(&left_ping, now + 110).unwrap();
        left.receive_ping(&right_ping, now + 10).unwrap();
        let (_, left_pong) = decode(&left.next_pong(now + 12).unwrap().unwrap());
        let (_, right_pong) = decode(&right.next_pong(now + 112).unwrap().unwrap());
        left.receive_pong(&right_pong, now + 22).unwrap();
        right.receive_pong(&left_pong, now + 122).unwrap();
        if round < 7 {
            assert!(left.estimate_event().is_none());
            assert!(right.estimate_event().is_none());
        }
    }
    let Some(MultiplayerEvent::ClockEstimated(left_estimate)) = left.estimate_event() else {
        panic!("eight exchanges must yield a left estimate")
    };
    let Some(MultiplayerEvent::ClockEstimated(right_estimate)) = right.estimate_event() else {
        panic!("eight exchanges must yield a right estimate")
    };
    assert!(left.estimate_event().is_none());
    assert!(right.estimate_event().is_none());
    assert!(left.next_ping(9_000).unwrap().is_none());
    assert!(right.next_ping(9_100).unwrap().is_none());
    (left_estimate, right_estimate)
}

#[test]
fn eight_symmetric_probes_preserve_both_clock_epochs_without_native_time() {
    let (left, right) = symmetric_estimates();
    assert_eq!(
        (
            left.lower_ns(),
            left.upper_ns(),
            left.round_trip_ns(),
            left.observed_local_ns()
        ),
        (90, 110, 20, 8_022)
    );
    assert_eq!(
        (
            right.lower_ns(),
            right.upper_ns(),
            right.round_trip_ns(),
            right.observed_local_ns()
        ),
        (-110, -90, 20, 8_122)
    );
    let deadline = right
        .remote_deadline_to_local(10_000, 9_100, 1_000)
        .unwrap();
    assert_eq!(
        (deadline.earliest_ns(), deadline.latest_ns()),
        (10_090, 10_110)
    );
    assert!(
        right
            .remote_deadline_to_local(10_000, 9_123, 1_000)
            .is_err()
    );
}

#[test]
fn invalid_probe_evidence_does_not_consume_a_valid_pending_exchange() {
    let mut left = ClockProbes::default();
    let mut right = ClockProbes::default();
    assert!(left.next_ping(-1).is_err());
    assert!(left.receive_pong(&[0; 32], 100).is_err());
    let (_, ping) = decode(&left.next_ping(100).unwrap().unwrap());
    assert!(left.next_ping(101).unwrap().is_none());
    assert!(right.receive_ping(&ping, -1).is_err());
    assert!(right.receive_ping(&ping[..15], 210).is_err());
    let mut negative_send = ping.clone();
    negative_send[8..].copy_from_slice(&(-1_i64).to_le_bytes());
    assert!(right.receive_ping(&negative_send, 210).is_err());
    right.receive_ping(&ping, 210).unwrap();
    assert!(right.receive_ping(&ping, 210).is_err());
    assert!(right.next_pong(209).is_err());
    let (_, pong) = decode(&right.next_pong(215).unwrap().unwrap());
    for offset in [0, 8] {
        let mut wrong = pong.clone();
        wrong[offset] ^= 1;
        assert!(left.receive_pong(&wrong, 125).is_err());
    }
    assert!(left.receive_pong(&pong[..31], 125).is_err());
    assert!(left.receive_pong(&pong, 99).is_err());
    assert!(left.receive_pong(&pong, 104).is_err()); // Peer processing exceeds local round trip.
    assert_eq!(left.completed, 0);
    assert_eq!(left.pending_ping, Some((0, 100)));
    left.receive_pong(&pong, 125).unwrap();
    assert_eq!(left.completed, 1);
    assert!(left.receive_pong(&pong, 125).is_err());
    assert!(left.next_ping(124).is_err());
    let (_, next) = decode(&left.next_ping(125).unwrap().unwrap());
    right.receive_ping(&next, 230).unwrap();
    assert_eq!(u64::from_le_bytes(next[..8].try_into().unwrap()), 1);
}

#[test]
fn actual_start_agreement_uses_shared_codec_and_commits_only_complete_frames() {
    let (host_estimate, join_estimate) = symmetric_estimates();
    let policy = StartPolicy {
        lead_ns: 1_000,
        min_remaining_ns: 100,
        max_age_ns: 10_000,
        max_uncertainty_ns: 20,
        max_release_lateness_ns: 25,
    };
    let mut host = StartAgreement::new_at(StartRole::Host, policy, 100).unwrap();
    let mut join = StartAgreement::new_at(StartRole::Join, policy, 200).unwrap();
    host.prepare(host_estimate).unwrap();
    join.prepare(join_estimate).unwrap();
    let host_ready = host.next(9_000).unwrap().unwrap();
    let join_ready = join.next(9_100).unwrap().unwrap();
    for (message, recipient, now) in [
        (host_ready, &mut join, 9_100),
        (join_ready, &mut host, 9_000),
    ] {
        let (tag, payload) = decode(&start_frame(message));
        recipient
            .receive(parse_start_frame(tag, &payload).unwrap(), now)
            .unwrap();
    }
    assert!(host.next(9_000).unwrap().is_none());
    host.written(host_ready, 9_000).unwrap();
    join.written(join_ready, 9_100).unwrap();
    let proposal = host.next(9_000).unwrap().unwrap();
    assert_eq!(proposal, StartMessage::Propose(10_200));
    let (tag, payload) = decode(&start_frame(proposal));
    assert!(parse_start_frame(tag, &payload[..7]).is_err());
    assert!(parse_start_frame(12, &payload).is_err());
    host.written(proposal, 9_001).unwrap();
    join.receive(parse_start_frame(tag, &payload).unwrap(), 9_101)
        .unwrap();
    let accept = join.next(9_101).unwrap().unwrap();
    assert_eq!(accept, StartMessage::Accept(10_200));
    join.written(accept, 9_102).unwrap();
    let (tag, payload) = decode(&start_frame(accept));
    host.receive(parse_start_frame(tag, &payload).unwrap(), 9_002)
        .unwrap();
    let commit = host.next(9_002).unwrap().unwrap();
    assert_eq!(commit, StartMessage::Commit(10_200));
    assert!(!host.committed());
    assert!(!join.committed());
    assert!(host.take_schedule().is_none());
    assert!(join.take_schedule().is_none());
    assert!(host.written(StartMessage::Commit(10_201), 9_003).is_err());
    host.written(commit, 9_003).unwrap();
    let (tag, payload) = decode(&start_frame(commit));
    join.receive(parse_start_frame(tag, &payload).unwrap(), 9_103)
        .unwrap();
    let host_schedule = host.take_schedule().unwrap();
    let join_schedule = join.take_schedule().unwrap();
    assert_eq!(
        (
            host_schedule.target_ns,
            host_schedule.song_target_ns,
            host_schedule.uncertainty_ns
        ),
        (10_100, 10_200, 20)
    );
    assert_eq!(
        (
            join_schedule.target_ns,
            join_schedule.song_target_ns,
            join_schedule.uncertainty_ns
        ),
        (10_100, 10_300, 20)
    );
    assert!(host.committed() && join.committed());
    assert!(host.take_schedule().is_none());
    assert!(join.take_schedule().is_none());
}
