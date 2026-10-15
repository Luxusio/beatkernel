// Actual transport bytes with one deliberately delayed public write receipt.
// Admission, clock, start and cancellation authority stay in RoomPlayClient.
use beatkernel_bms_runtime::{
    multiplayer_protocol::OutboundFrame,
    multiplayer_room_play::{RoomPlayClient, RoomPlayError},
    multiplayer_room_wire::{decode_message, RoomFrameDecoder, RoomMessage},
    multiplayer_start::StartMessage,
};
use std::io::{Read, Write};

#[derive(Clone, Copy, Debug)]
struct AcceptReceipt {
    id: u64,
    target: i64,
    completed_ns: i64,
}

struct RawPeer {
    client: Option<RoomPlayClient>,
    stream: Option<WebTransportStream>,
    origin: Instant,
    last_observed: Option<i64>,
    pending: Option<OutboundFrame>,
    offset: usize,
    decoder: RoomFrameDecoder,
    // Increment only after exact push admission; reset only after take(Some).
    decoder_bytes: usize,
    hold_accept: bool,
    held: Option<AcceptReceipt>,
    actual_commit: Option<(RoomMessage, i64)>,
    schedule: Option<StartSchedule>,
    schedule_taken_ns: Option<i64>,
}

fn raw_error(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

impl RawPeer {
    fn connect(
        config: &WebTransportOptions,
        identity: &[u8],
        players: &[PlayerId],
        index: usize,
    ) -> Self {
        let client =
            RoomPlayClient::new(identity, players, StartPolicy::default(), PREROLL[index]).unwrap();
        let stream = WebTransportEndpoint::prepare(config)
            .unwrap()
            .connect(&AtomicBool::new(false), Instant::now() + WAIT)
            .expect("actual HTTP/3 raw room stream setup failed");
        Self {
            client: Some(client),
            stream: Some(stream),
            origin: Instant::now(),
            last_observed: None,
            pending: None,
            offset: 0,
            decoder: RoomFrameDecoder::new(),
            decoder_bytes: 0,
            hold_accept: index == 1,
            held: None,
            actual_commit: None,
            schedule: None,
            schedule_taken_ns: None,
        }
    }

    fn owner(&self) -> &RoomPlayClient {
        self.client.as_ref().unwrap()
    }

    fn owner_mut(&mut self) -> &mut RoomPlayClient {
        self.client.as_mut().unwrap()
    }

    fn now(&mut self) -> io::Result<i64> {
        let now = i64::try_from(self.origin.elapsed().as_nanos())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "elapsed clock overflow"))?;
        if self.last_observed.is_some_and(|previous| now < previous) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "elapsed clock regressed",
            ));
        }
        self.last_observed = Some(now);
        Ok(now)
    }

    fn capture_schedule(&mut self) -> io::Result<()> {
        if let Some(schedule) = self.owner_mut().take_schedule() {
            let taken_ns = self.now()?;
            assert!(
                self.schedule.replace(schedule).is_none(),
                "duplicate committed start"
            );
            assert!(self.schedule_taken_ns.replace(taken_ns).is_none());
        }
        Ok(())
    }

    // One actual write and one decoder-needed-prefix read at most. There is no
    // retry loop, clock substitution, transport acknowledgement or policy copy.
    fn step(&mut self) -> io::Result<bool> {
        let polled_ns = self.now()?;
        if self.pending.is_none() {
            self.pending = self.owner_mut().poll_write(polled_ns).map_err(raw_error)?;
        }
        let mut moved = false;
        if let Some(frame) = &self.pending {
            let remaining = frame.bytes.get(self.offset..).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "raw write offset exceeded frame",
                )
            })?;
            let remaining_len = remaining.len();
            let frame_len = frame.bytes.len();
            let id = frame.id;
            let target = match decode_message(&frame.bytes).map_err(raw_error)? {
                RoomMessage::Start(StartMessage::Accept(target)) => Some(target),
                _ => None,
            };
            match self.stream.as_mut().unwrap().write(remaining) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "raw write made no progress",
                    ))
                }
                Ok(count) => {
                    let completed_ns = self.now()?;
                    if count > remaining_len {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "raw writer exceeded slice",
                        ));
                    }
                    self.offset += count;
                    moved = true;
                    if self.offset == frame_len {
                        if self.hold_accept && target.is_some() {
                            assert!(self.held.is_none());
                            self.held = Some(AcceptReceipt {
                                id,
                                target: target.unwrap(),
                                completed_ns,
                            });
                            self.hold_accept = false;
                        } else {
                            let processing_ns = self.now()?;
                            self.owner_mut()
                                .written_at(id, completed_ns, processing_ns)
                                .map_err(raw_error)?;
                        }
                        self.pending = None;
                        self.offset = 0;
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error),
            }
        }

        let needed = self.decoder.needed().map_err(raw_error)?;
        if needed == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "raw decoder retained a complete frame",
            ));
        }
        let mut scratch = [0_u8; 4096];
        let limit = needed.min(scratch.len());
        match self.stream.as_mut().unwrap().read(&mut scratch[..limit]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "raw room stream ended",
                ))
            }
            Ok(count) => {
                let captured_ns = self.now()?;
                if count > limit {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "raw reader exceeded slice",
                    ));
                }
                let admitted = self.decoder.push(&scratch[..count]).map_err(raw_error)?;
                if admitted != count {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "raw decoder did not admit exact prefix",
                    ));
                }
                self.decoder_bytes = self.decoder_bytes.checked_add(count).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "raw decoder byte count overflow",
                    )
                })?;
                moved = true;
                if let Some(message) = self.decoder.take().map_err(raw_error)? {
                    self.decoder_bytes = 0;
                    let commit = match &message {
                        RoomMessage::Start(StartMessage::Commit(target)) if self.held.is_some() => {
                            assert_eq!(
                                *target,
                                self.held.unwrap().target,
                                "actual Commit must match actual Accept"
                            );
                            Some(message.clone())
                        }
                        _ => None,
                    };
                    let processing_ns = self.now()?;
                    self.owner_mut()
                        .receive_at(message, captured_ns, processing_ns)
                        .map_err(raw_error)?;
                    if let Some(commit) = commit {
                        assert!(
                            self.held.is_some(),
                            "callback must still be held during receive_at"
                        );
                        assert!(self.actual_commit.replace((commit, captured_ns)).is_none());
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
        self.capture_schedule()?;
        Ok(moved)
    }

    fn clean(&self) -> bool {
        self.pending.is_none() && self.offset == 0 && self.held.is_none() && self.decoder_bytes == 0
    }

    fn into_peer(mut self) -> Peer {
        // A newly polled frame at offset zero is still outstanding authority.
        assert!(self.pending.is_none());
        assert_eq!(self.offset, 0);
        assert!(self.held.is_none());
        assert_eq!(self.decoder_bytes, 0);
        Peer {
            io: Some(RoomPlayIo::new(
                self.client.take().unwrap(),
                self.stream.take().unwrap(),
            )),
            retired: None,
            origin: self.origin,
            schedule: self.schedule,
        }
    }

    fn close(&mut self) -> io::Result<()> {
        if let Some(client) = &mut self.client {
            client.stop();
        }
        match self.stream.take() {
            Some(mut stream) => stream.finish(CLOSE),
            None => Ok(()),
        }
    }
}

impl Drop for RawPeer {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn raw_pump(peers: &mut [RawPeer], label: &str, done: impl Fn(&[RawPeer]) -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done(peers) {
        assert!(Instant::now() < deadline, "timed out waiting for {label}");
        for peer in peers.iter_mut() {
            peer.step()
                .unwrap_or_else(|error| panic!("{label}: {error:?}"));
        }
    }
}

fn prepared_history(
    client: &RoomPlayClient,
    roster: &[GroupRoomMember],
    index: usize,
    identity: &[u8],
) {
    assert_eq!(client.participant(), Some(roster[index].id));
    let room = client.room().unwrap();
    assert_eq!(room.identity, identity);
    assert_eq!(room.phase, GroupRoomPhase::Prepared);
    assert_eq!(room.members, roster);
    assert_eq!(room.deadline_ns, None);
    assert!(!client.leave_requested());
    assert!(!client.leave_written());
    assert!(!client.local_final_written());
    assert!(!client.local_final_acknowledged());
    assert!(!client.progress_complete());
    assert!(!client.drain_complete());
    for member in roster {
        assert!(client.peer_progress(member.id).is_none());
        assert!(!client.peer_final_ack_written(member.id));
    }
}

fn assert_schedule(peer: &mut RawPeer, index: usize) {
    let schedule = peer
        .schedule
        .expect("actual Commit must deliver a schedule");
    let taken_ns = peer.schedule_taken_ns.unwrap();
    // Compare each schedule with its original local elapsed domain only.
    assert!(schedule.target_ns > taken_ns);
    assert!(schedule.song_target_ns > 0);
    assert_eq!(
        schedule.song_target_ns.checked_sub(schedule.target_ns),
        Some(PREROLL[index])
    );
    assert!(schedule.uncertainty_ns <= StartPolicy::default().max_uncertainty_ns);
    assert_eq!(peer.owner_mut().take_schedule(), None);
}

fn actual_commit_with_held_receipt(
    config: &WebTransportOptions,
    identities: &[Vec<u8>],
    players: &[[PlayerId; 2]; 3],
) -> (Vec<RawPeer>, Vec<GroupRoomMember>) {
    assert_eq!(identities.len(), players.len());
    assert!(identities.iter().all(|identity| identity == &identities[0]));
    let mut peers = Vec::new();
    let mut roster: Vec<GroupRoomMember> = Vec::new();
    let mut deadlines = Vec::new();
    for (index, (identity, players)) in identities.iter().zip(players).enumerate() {
        peers.push(RawPeer::connect(config, identity, players, index));
        raw_pump(&mut peers, "raw actual Join admission", |peers| {
            peers.last().unwrap().owner().participant().is_some()
                && peers.last().unwrap().owner().room().is_some()
        });
        let client = peers[index].owner();
        let id = client.participant().unwrap();
        assert_ne!(id.0, 0);
        assert!(roster.iter().all(|member| member.id != id));
        deadlines.push(client.room().unwrap().deadline_ns.unwrap());
        roster.push(GroupRoomMember {
            id,
            players: players.to_vec(),
            prepared: false,
        });
        raw_pump(&mut peers, "raw exact ordered Collecting roster", |peers| {
            peers.iter().all(|peer| {
                peer.owner().room().is_some_and(|room| {
                    room.phase == GroupRoomPhase::Collecting && room.members == roster.as_slice()
                })
            })
        });
        for (index, peer) in peers.iter_mut().enumerate() {
            let room = peer.owner().room().unwrap();
            assert_eq!(room.identity, identities[index].as_slice());
            assert_eq!(room.deadline_ns, Some(deadlines[index]));
            assert!(peer.schedule.is_none());
            assert_eq!(
                peer.owner_mut()
                    .publish_progress(&members(11 + index as u32 * 10, 1), false),
                Err(RoomPlayError::InvalidState)
            );
        }
    }
    for peer in &mut peers[1..] {
        assert!(peer.owner_mut().request_seal().is_err());
    }
    peers[0].owner_mut().request_seal().unwrap();
    raw_pump(
        &mut peers,
        "raw creator Seal and exact Frozen roster",
        |peers| {
            peers.iter().all(|peer| {
                peer.owner().room().is_some_and(|room| {
                    room.phase == GroupRoomPhase::Frozen && room.members == roster.as_slice()
                })
            })
        },
    );
    for ready in 0..2 {
        peers[ready].owner_mut().request_ready().unwrap();
        roster[ready].prepared = true;
        raw_pump(&mut peers, "raw each positive partial Ready", |peers| {
            peers.iter().all(|peer| {
                peer.owner().room().is_some_and(|room| {
                    room.phase == GroupRoomPhase::Frozen && room.members == roster.as_slice()
                })
            })
        });
        for (index, peer) in peers.iter_mut().enumerate() {
            assert_eq!(
                peer.owner().room().unwrap().deadline_ns,
                Some(deadlines[index])
            );
            assert!(peer.schedule.is_none());
            assert_eq!(
                peer.owner_mut()
                    .publish_progress(&members(11 + index as u32 * 10, 1), false),
                Err(RoomPlayError::InvalidState)
            );
        }
    }
    peers[2].owner_mut().request_ready().unwrap();
    roster[2].prepared = true;
    raw_pump(
        &mut peers,
        "actual matching Commit while exact Accept receipt is held",
        |peers| {
            peers.iter().all(|peer| {
                peer.owner().room().is_some_and(|room| {
                    room.phase == GroupRoomPhase::Prepared
                        && room.members == roster.as_slice()
                        && room.deadline_ns.is_none()
                })
            }) && peers[1].actual_commit.is_some()
                && peers[1].held.is_some()
                && peers[0].schedule.is_some()
                && peers[2].schedule.is_some()
                && peers[0].clean()
                && peers[2].clean()
                && peers[1].pending.is_none()
                && peers[1].offset == 0
                && peers[1].decoder_bytes == 0
        },
    );
    let victim = &mut peers[1];
    let receipt = victim.held.unwrap();
    assert_eq!(
        victim.actual_commit.as_ref().unwrap().0,
        RoomMessage::Start(StartMessage::Commit(receipt.target))
    );
    assert!(receipt.completed_ns <= victim.actual_commit.as_ref().unwrap().1);
    assert!(victim.schedule.is_none());
    assert_eq!(victim.owner_mut().take_schedule(), None);
    let polled_ns = victim.now().unwrap();
    assert!(victim.owner_mut().poll_write(polled_ns).unwrap().is_none());
    // A valid poll advances last_now; full immutability is asserted after Stop.
    assert_eq!(
        victim.owner_mut().publish_progress(&members(21, 1), false),
        Err(RoomPlayError::InvalidState)
    );
    assert_eq!(
        victim.owner_mut().request_leave(),
        Err(RoomPlayError::InvalidState)
    );
    for (index, peer) in peers.iter_mut().enumerate() {
        prepared_history(peer.owner(), &roster, index, &identities[index]);
        if index != 1 {
            assert_schedule(peer, index);
        }
    }
    (peers, roster)
}

fn assert_stopped_receipt(peer: &mut RawPeer, stopped: &RoomPlayClient) {
    let receipt = peer.held.unwrap();
    let (commit, captured_ns) = peer.actual_commit.as_ref().unwrap().clone();
    let now = peer.now().unwrap();
    macro_rules! stopped {
        ($operation:expr) => {
            assert_eq!($operation.unwrap_err(), RoomPlayError::Stopped);
            assert_eq!(peer.owner(), stopped);
        };
    }
    stopped!(peer
        .owner_mut()
        .written_at(receipt.id, receipt.completed_ns, now));
    stopped!(peer.owner_mut().written(receipt.id, now));
    stopped!(peer.owner_mut().poll_write(now));
    // This is a duplicate public API replay of retained actual input, not a
    // claim that a second Commit packet arrived from the network.
    stopped!(peer
        .owner_mut()
        .receive_at(commit.clone(), captured_ns, now));
    stopped!(peer.owner_mut().receive(commit, now));
    stopped!(peer.owner_mut().request_seal());
    stopped!(peer.owner_mut().request_ready());
    stopped!(peer.owner_mut().request_leave());
    stopped!(peer.owner_mut().request_drain());
    stopped!(peer.owner_mut().publish_progress(&members(21, 1), false));
    stopped!(peer.owner_mut().publish_progress(&members(21, 3), true));
    assert_eq!(peer.owner_mut().take_schedule(), None);
    assert_eq!(peer.owner(), stopped);
    peer.owner_mut().stop();
    assert_eq!(peer.owner(), stopped);
    assert!(peer.schedule.is_none());
}

#[test]
#[ignore = "requires local HTTP/3 relay and ephemeral valid PKI; actual delayed Accept receipt"]
fn real_three_host_delayed_accept_receipt_releases_one_schedule_after_actual_commit() {
    let selected = Selected::default();
    let players = [
        [PlayerId(11), PlayerId(12)],
        [PlayerId(21), PlayerId(22)],
        [PlayerId(31), PlayerId(32)],
    ];
    let identities: Vec<_> = players
        .iter()
        .map(|players| admitted_identity(selected, players))
        .collect();
    let config = options("delayed_accept_receipt");
    let (mut peers, _) = actual_commit_with_held_receipt(&config, &identities, &players);
    let victim = &mut peers[1];
    let receipt = victim.held.unwrap();
    // Release immediately, retaining the actual complete-write observation and
    // a separately captured current processing time. WAIT is only an outer bound.
    let processing_ns = victim.now().unwrap();
    victim
        .owner_mut()
        .written_at(receipt.id, receipt.completed_ns, processing_ns)
        .unwrap();
    victim.held = None;
    victim.capture_schedule().unwrap();
    assert_schedule(victim, 1);
    raw_pump(
        &mut peers,
        "clean raw owners after the original Accept receipt",
        |peers| peers.iter().all(RawPeer::clean),
    );
    complete_progress_and_drain(peers.into_iter().map(RawPeer::into_peer).collect());
}

#[test]
#[ignore = "requires local HTTP/3 relay and ephemeral valid PKI; actual Commit Stop and replacement"]
fn real_three_host_stop_after_actual_commit_fences_late_accept_receipt_and_replacement() {
    let selected = Selected::default();
    let players = [
        [PlayerId(11), PlayerId(12)],
        [PlayerId(21), PlayerId(22)],
        [PlayerId(31), PlayerId(32)],
    ];
    let identities: Vec<_> = players
        .iter()
        .map(|players| admitted_identity(selected, players))
        .collect();
    let replacement_identities: Vec<_> = players
        .iter()
        .map(|players| {
            admitted_identity(
                Selected {
                    seed: selected.seed + 1,
                    ..selected
                },
                players,
            )
        })
        .collect();
    assert!(replacement_identities
        .iter()
        .all(|identity| identity == &replacement_identities[0]));
    assert_ne!(identities[0], replacement_identities[0]);
    let config = options("stopped_accept_receipt");
    let (mut old, roster) = actual_commit_with_held_receipt(&config, &identities, &players);
    let mut victim = old.remove(1);
    victim.owner_mut().stop();
    let stopped = victim.owner().clone();
    assert_stopped_receipt(&mut victim, &stopped);
    prepared_history(victim.owner(), &roster, 1, &identities[1]);
    let mut survivors: Vec<_> = [0, 2]
        .into_iter()
        .zip(old)
        .map(|(index, peer)| (index, peer.into_peer()))
        .collect();
    let schedules: Vec<_> = survivors.iter().map(|(_, peer)| peer.schedule).collect();
    // Local Stop/Drop alone does not drive a current-thread QUIC runtime.
    // Retain the original finish result until all old closes have been tried.
    let started = Instant::now();
    let victim_finish = victim.stream.as_mut().unwrap().finish(CLOSE);
    let victim_elapsed = started.elapsed();
    let deadline = Instant::now() + WAIT;
    let mut first_errors: [Option<io::Error>; 2] = [None, None];
    while first_errors.iter().any(Option::is_none) {
        assert!(
            Instant::now() < deadline,
            "both survivors must observe their own transport retirement"
        );
        for (slot, (_, peer)) in survivors.iter_mut().enumerate() {
            if first_errors[slot].is_none() {
                if let Err(error) = peer.step() {
                    first_errors[slot] = Some(error);
                }
            }
        }
    }
    for (slot, (index, peer)) in survivors.iter_mut().enumerate() {
        let first = first_errors[slot].as_ref().unwrap();
        assert!(
            matches!(
                first.kind(),
                io::ErrorKind::UnexpectedEof
                    | io::ErrorKind::Other
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::BrokenPipe
            ),
            "expected actual non-timeout transport retirement, got {first}"
        );
        assert_fenced(peer, *index, first);
        prepared_history(peer.owner().session(), &roster, *index, &identities[*index]);
        assert_eq!(peer.schedule, schedules[slot]);
        assert!(peer.schedule.is_some());
    }
    // Reuse this exact config/URL while the old stopped owner and every old
    // stream are retained, with genuinely different selected prepared identity.
    let (mut replacement, mut new_roster, new_deadlines) =
        collecting_with_identities(&config, &replacement_identities);
    assert!(victim.stream.is_some());
    assert!(survivors.iter().all(|(_, peer)| peer.io.is_some()));
    for member in &new_roster {
        assert_ne!(member.id.0, 0);
        assert!(roster.iter().all(|old_member| old_member.id != member.id));
    }
    assert_stopped_receipt(&mut victim, &stopped);
    prepared_history(victim.owner(), &roster, 1, &identities[1]);
    for (slot, (index, peer)) in survivors.iter_mut().enumerate() {
        assert_fenced(peer, *index, first_errors[slot].as_ref().unwrap());
        prepared_history(peer.owner().session(), &roster, *index, &identities[*index]);
        assert_eq!(peer.schedule, schedules[slot]);
    }
    let started = Instant::now();
    let mut closes: Vec<_> = survivors.iter_mut().map(|(_, peer)| peer.close()).collect();
    closes.push(victim.close());
    let close_elapsed = started.elapsed();
    // Every old close is attempted before inspecting any result. Public
    // drain.and(closed) may mask a later idle timeout after an earlier reset;
    // this asserts returned errors/bounds, not internal graceful completion.
    for result in &closes {
        if let Err(error) = result {
            assert_ne!(error.kind(), io::ErrorKind::TimedOut, "old close: {error}");
        }
    }
    assert!(victim_elapsed < WAIT, "victim shutdown exceeded its bound");
    assert!(
        close_elapsed < WAIT,
        "all old close attempts exceeded their bound"
    );
    match (&victim_finish, &closes[2]) {
        (Ok(()), Ok(())) => {}
        (Err(first), Err(repeated)) => {
            assert_ne!(
                first.kind(),
                io::ErrorKind::TimedOut,
                "victim finish: {first}"
            );
            assert_eq!(repeated.kind(), first.kind());
            assert_eq!(repeated.to_string(), first.to_string());
        }
        (first, repeated) => panic!("victim finish result changed: {first:?}, {repeated:?}"),
    }
    assert!(victim.stream.is_none());
    assert!(survivors
        .iter()
        .all(|(_, peer)| peer.io.is_none() && peer.retired.is_none()));
    assert_eq!(victim.owner(), &stopped);
    frozen_partial_ready(&mut replacement, &mut new_roster, &new_deadlines);
    commit_last_ready(&mut replacement, &mut new_roster);
    complete_progress_and_drain(replacement);
}
