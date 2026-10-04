//! Actual BKMR admission and measured software-start ownership.
//! Progress, gameplay and final application ACK traffic remain unsupported.

use super::*;
use crate::local_players::PlayerId;
use crate::multiplayer_group_rooms::{
    GroupParticipantTicket, GroupRoomMember, GroupRoomPhase, GroupRoomPolicy, GroupRoomRegistry,
    GroupRoomSnapshot,
};
use crate::multiplayer_room_clock::RoomClockExchange;
use crate::multiplayer_room_start::RoomStartCoordinator;
use crate::multiplayer_room_wire::{RoomFrameDecoder, RoomMessage, encode_message};
use crate::multiplayer_start::{StartMessage, StartPolicy};
use tokio::sync::mpsc;

const OUTGOING_CAPACITY: usize = 4;
const COMMAND_CAPACITY: usize = 256;
#[derive(Clone, Debug)]
struct QueuedFrame {
    bytes: Arc<Vec<u8>>,
    receipt: Option<u64>,
}
type Frames = BTreeMap<ParticipantId, mpsc::Sender<QueuedFrame>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ControlReceipt {
    Clock(u64),
    Start(StartMessage),
}

struct PeerControl {
    id: ParticipantId,
    clock: RoomClockExchange,
    estimate_installed: bool,
    next_id: Option<u64>,
    in_flight: Option<(u64, ControlReceipt, i64)>,
    pending_accept: Option<StartMessage>,
    last_received: Option<i64>,
}

/// The same bounded control owner is called by the live actor and portable
/// channel fixtures. Queue/transport failures require whole-room disposal.
struct PreparedRoom {
    coordinator: RoomStartCoordinator,
    peers: Vec<PeerControl>,
    prepared_now: i64,
    deadline: i64,
    last_now: i64,
}

impl PreparedRoom {
    fn new(snapshot: GroupRoomSnapshot<'_>, now: i64, timeout_ns: i64) -> io::Result<Self> {
        if now < 0 || timeout_ns <= 0 {
            return Err(invalid("invalid prepared room deadline"));
        }
        let deadline = now
            .checked_add(timeout_ns)
            .ok_or_else(|| invalid("prepared room deadline overflow"))?;
        let coordinator =
            RoomStartCoordinator::new(snapshot, StartPolicy::default()).map_err(invalid)?;
        let mut peers = Vec::new();
        peers
            .try_reserve_exact(snapshot.members.len())
            .map_err(invalid)?;
        for member in snapshot.members {
            peers.push(PeerControl {
                id: member.id,
                clock: RoomClockExchange::new(snapshot, member.id).map_err(invalid)?,
                estimate_installed: false,
                next_id: Some(1),
                in_flight: None,
                pending_accept: None,
                last_received: None,
            });
        }
        Ok(Self {
            coordinator,
            peers,
            prepared_now: now,
            deadline,
            last_now: now,
        })
    }

    fn validate_now(&self, now: i64) -> io::Result<()> {
        if now < self.last_now {
            return Err(invalid("prepared room processing clock regressed"));
        }
        if self.expired(now) {
            return Err(elapsed_timeout());
        }
        Ok(())
    }

    fn peer_index(&self, id: ParticipantId) -> io::Result<usize> {
        self.peers
            .iter()
            .position(|peer| peer.id == id)
            .ok_or_else(|| invalid("unknown prepared room lease"))
    }

    fn receive(
        &mut self,
        id: ParticipantId,
        message: &RoomMessage,
        captured_ns: i64,
        now: i64,
    ) -> io::Result<()> {
        self.validate_now(now)?;
        let index = self.peer_index(id)?;
        let peer = &mut self.peers[index];
        if captured_ns < self.prepared_now
            || captured_ns > now
            || peer
                .last_received
                .is_some_and(|previous| captured_ns < previous)
        {
            return Err(invalid("invalid prepared room read observation"));
        }
        match message {
            RoomMessage::ClockPing { .. } | RoomMessage::ClockPong { .. } => {
                peer.clock
                    .receive_at(message, captured_ns, now)
                    .map_err(invalid)?;
            }
            RoomMessage::Start(message) => {
                if let (
                    StartMessage::Accept(target),
                    Some((_, ControlReceipt::Start(StartMessage::Propose(expected)), admitted_ns)),
                ) = (*message, peer.in_flight)
                {
                    if peer.pending_accept.is_some()
                        || target != expected
                        || captured_ns < admitted_ns
                    {
                        return Err(invalid("unexpected early room Accept"));
                    }
                    let mut candidate = self.coordinator.clone();
                    candidate
                        .written(id, StartMessage::Propose(expected), now)
                        .map_err(invalid)?;
                    candidate.receive(id, *message, now).map_err(invalid)?;
                    peer.pending_accept = Some(*message);
                } else {
                    if peer.pending_accept.is_some() {
                        return Err(invalid("duplicate pending room start response"));
                    }
                    self.coordinator
                        .receive(id, *message, now)
                        .map_err(invalid)?;
                }
            }
            _ => return Err(invalid("unexpected prepared room control")),
        }
        peer.last_received = Some(captured_ns);
        self.last_now = now;
        Ok(())
    }

    fn written(
        &mut self,
        id: ParticipantId,
        write_id: u64,
        completed_ns: i64,
        now: i64,
    ) -> io::Result<()> {
        self.validate_now(now)?;
        let index = self.peer_index(id)?;
        let peer = &mut self.peers[index];
        let (expected, receipt, admitted_ns) = peer
            .in_flight
            .ok_or_else(|| invalid("unexpected room control write receipt"))?;
        if expected != write_id {
            return Err(invalid("room control write identity mismatch"));
        }
        if completed_ns < admitted_ns || completed_ns > now {
            return Err(invalid("invalid room control write observation"));
        }
        match receipt {
            ControlReceipt::Clock(inner) => peer
                .clock
                .written_at(inner, completed_ns, now)
                .map_err(invalid)?,
            ControlReceipt::Start(message) => {
                let mut candidate = self.coordinator.clone();
                candidate.written(id, message, now).map_err(invalid)?;
                if let Some(accept) = peer.pending_accept {
                    candidate.receive(id, accept, now).map_err(invalid)?;
                }
                self.coordinator = candidate;
                peer.pending_accept = None;
            }
        }
        peer.in_flight = None;
        self.last_now = now;
        Ok(())
    }

    /// Pump after actual input/completion events. A failure after a prior peer
    /// queued a frame closes the whole room; it cannot retract transmitted bytes.
    fn pump(&mut self, frames: &Frames, now: i64) -> io::Result<()> {
        self.validate_now(now)?;
        for peer in &mut self.peers {
            if peer.in_flight.is_some() {
                continue;
            }
            if !peer.estimate_installed {
                if let Some(estimate) = peer.clock.estimate() {
                    self.coordinator
                        .prepare(peer.id, estimate, now)
                        .map_err(invalid)?;
                    peer.estimate_installed = true;
                }
            }
            let mut clock = peer.clock.clone();
            let next = if let Some(frame) = clock.next(now).map_err(invalid)? {
                Some((frame.bytes, ControlReceipt::Clock(frame.id)))
            } else if let Some(message) = self.coordinator.next(peer.id, now).map_err(invalid)? {
                Some((
                    encode_message(&RoomMessage::Start(message)).map_err(invalid)?,
                    ControlReceipt::Start(message),
                ))
            } else {
                None
            };
            if let Some((bytes, receipt)) = next {
                let id = peer
                    .next_id
                    .ok_or_else(|| invalid("room control write identity exhausted"))?;
                frames
                    .get(&peer.id)
                    .ok_or_else(|| invalid("prepared room output lease missing"))?
                    .try_send(QueuedFrame {
                        bytes: Arc::new(bytes),
                        receipt: Some(id),
                    })
                    .map_err(|_| invalid("prepared room output full or closed"))?;
                peer.next_id = id.checked_add(1);
                peer.in_flight = Some((id, receipt, now));
            }
            peer.clock = clock;
        }
        self.last_now = now;
        Ok(())
    }

    fn expired(&self, now: i64) -> bool {
        !self.committed() && now >= self.deadline
    }

    fn committed(&self) -> bool {
        self.coordinator.committed()
    }
}

/// Idle reads await the original first byte without an I/O deadline. Once it
/// arrives, one finite deadline covers the rest of this complete BKMR frame.
async fn read_message<R: AsyncRead + Unpin>(
    read: &mut R,
    limit: Duration,
    stop: &mut watch::Receiver<bool>,
) -> io::Result<Option<RoomMessage>> {
    if limit.is_zero() || limit > Duration::from_secs(120) {
        return Err(invalid("invalid room I/O timeout"));
    }
    tokio::select! {
        biased;
        _ = wait_stop(stop) => Err(cancelled()),
        result = async {
            let mut first = [0u8; 1];
            if read.read(&mut first).await? == 0 { return Ok(None); }
            let deadline = Instant::now() + limit;
            let mut decoder = RoomFrameDecoder::new();
            decoder.push(&first).map_err(invalid)?;
            timeout_at(deadline, async {
                let mut scratch = [0u8; 4096];
                loop {
                    if let Some(message) = decoder.take().map_err(invalid)? { return Ok(Some(message)); }
                    let needed = decoder.needed().map_err(invalid)?;
                    let count = read.read(&mut scratch[..needed.min(4096)]).await?;
                    if count == 0 {
                        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "truncated room frame"));
                    }
                    decoder.push(&scratch[..count]).map_err(invalid)?;
                }
            }).await.map_err(|_| elapsed_timeout())?
        } => result,
    }
}

#[derive(Debug)]
struct ServerControlRead {
    message: RoomMessage,
    captured_ns: i64,
}

async fn read_control<R: AsyncRead + Unpin>(
    read: &mut R,
    limit: Duration,
    stop: &mut watch::Receiver<bool>,
    origin: Instant,
) -> io::Result<Option<ServerControlRead>> {
    let Some(message) = read_message(read, limit, stop).await? else {
        return Ok(None);
    };
    let captured_ns = now(origin)?;
    Ok(Some(ServerControlRead {
        message,
        captured_ns,
    }))
}

/// Frames originate from the actual room codec. Each queue is bounded, and a
/// completed write is only local transport progress, never a room/gameplay ACK.
async fn write_frames<W: AsyncWrite + Unpin>(
    mut write: W,
    mut frames: mpsc::Receiver<QueuedFrame>,
    limit: Duration,
    mut stop: watch::Receiver<bool>,
    id: ParticipantId,
    origin: Instant,
    commands: mpsc::Sender<Command>,
) -> io::Result<()> {
    if limit.is_zero() || limit > Duration::from_secs(120) {
        return Err(invalid("invalid room I/O timeout"));
    }
    tokio::select! {
        biased;
        _ = wait_stop(&mut stop) => Err(cancelled()),
        result = async {
            while let Some(frame) = frames.recv().await {
                timeout(limit, write.write_all(frame.bytes.as_slice())).await.map_err(|_| elapsed_timeout())??;
                if let Some(write_id) = frame.receipt {
                    let completed_ns = now(origin)?;
                    commands.send(Command {
                        id,
                        event: PeerEvent::Written { write_id, completed_ns },
                    }).await.map_err(|_| cancelled())?;
                }
            }
            Ok(())
        } => result,
    }
}

/// Apply only stream-bound participant requests. Responses and a second Join
/// are protocol errors; no message can choose another participant's lease.
fn apply_request(
    registry: &mut GroupRoomRegistry,
    participant: ParticipantId,
    request: &RoomMessage,
    time: i64,
) -> io::Result<Vec<GroupParticipantTicket>> {
    match request {
        RoomMessage::Seal => registry.seal(participant, time).map_err(invalid)?,
        RoomMessage::Ready => {
            registry.ready(participant, time).map_err(invalid)?;
        }
        RoomMessage::Leave => return registry.release(participant, time).map_err(invalid),
        _ => return Err(invalid("unexpected message after room admission")),
    }
    Ok(Vec::new())
}

/// Encode once, then reserve one slot for every host before publishing any row.
/// A closed/full host refuses the complete broadcast without blocking the owner.
fn publish_room(registry: &GroupRoomRegistry, frames: &Frames, key: &str) -> io::Result<()> {
    let room = registry
        .room(key)
        .ok_or_else(|| invalid("missing group room"))?;
    let mut members = Vec::new();
    members
        .try_reserve_exact(room.members.len())
        .map_err(invalid)?;
    for member in room.members {
        let mut players = Vec::new();
        players
            .try_reserve_exact(member.players.len())
            .map_err(invalid)?;
        players.extend_from_slice(&member.players);
        members.push(GroupRoomMember {
            id: member.id,
            players,
            prepared: member.prepared,
        });
    }
    let frame = Arc::new(
        encode_message(&RoomMessage::Snapshot {
            members,
            phase: room.phase,
            deadline_ns: room.deadline_ns,
        })
        .map_err(invalid)?,
    );
    let mut slots = Vec::new();
    slots
        .try_reserve_exact(room.members.len())
        .map_err(invalid)?;
    for member in room.members {
        let sender = frames
            .get(&member.id)
            .ok_or_else(|| invalid("room output lease missing"))?;
        slots.push(
            sender
                .try_reserve()
                .map_err(|_| invalid("room output full or closed"))?,
        );
    }
    for slot in slots {
        slot.send(QueuedFrame {
            bytes: frame.clone(),
            receipt: None,
        });
    }
    Ok(())
}

struct GroupPrepared {
    prepared: Prepared,
    identity: Vec<u8>,
    players: Vec<PlayerId>,
}

async fn prepare_join(
    incoming: wtransport::endpoint::IncomingSession,
    permit: OwnedSemaphorePermit,
    options: Arc<ServerOptions>,
    deadline: Instant,
    mut stop: watch::Receiver<bool>,
) -> io::Result<GroupPrepared> {
    // The outer absolute deadline includes parent TLS/session/stream setup and
    // first Join bytes. Parent preparation cannot grant a second setup period.
    timeout_at(deadline, async {
        let mut prepared = prepare(incoming, permit, options.clone()).await?;
        let (_, read) = prepared
            .resource
            .stream
            .as_mut()
            .ok_or_else(|| invalid("prepared stream missing"))?;
        match read_message(read, options.io_timeout, &mut stop).await? {
            Some(RoomMessage::Join { identity, players }) => Ok(GroupPrepared {
                prepared,
                identity,
                players,
            }),
            _ => Err(invalid("first room frame must be Join")),
        }
    })
    .await
    .map_err(|_| elapsed_timeout())?
}

#[derive(Debug)]
struct Command {
    id: ParticipantId,
    event: PeerEvent,
}

#[derive(Debug)]
enum PeerEvent {
    Received(ServerControlRead),
    Written { write_id: u64, completed_ns: i64 },
}

/// One joined task owns both stream futures and their original Resource. The
/// resource is returned on completion so its session permit survives until join.
async fn peer_io(
    id: ParticipantId,
    mut read: RecvStream,
    write: SendStream,
    commands: mpsc::Sender<Command>,
    frames: mpsc::Receiver<QueuedFrame>,
    limit: Duration,
    mut stop: watch::Receiver<bool>,
    origin: Instant,
) -> io::Result<()> {
    let mut reading_stop = stop.clone();
    let writing_stop = stop.clone();
    let reader = async {
        while let Some(observation) =
            read_control(&mut read, limit, &mut reading_stop, origin).await?
        {
            commands
                .send(Command {
                    id,
                    event: PeerEvent::Received(observation),
                })
                .await
                .map_err(|_| cancelled())?;
        }
        Ok(())
    };
    let writer = write_frames(
        write,
        frames,
        limit,
        writing_stop,
        id,
        origin,
        commands.clone(),
    );
    tokio::pin!(reader, writer);
    tokio::select! {
        biased;
        _ = wait_stop(&mut stop) => Err(cancelled()),
        result = &mut reader => result,
        result = &mut writer => result,
    }
}

struct PeerResource {
    key: String,
    connection: Connection,
    stop: watch::Sender<bool>,
    last_received: Option<i64>,
}
impl Drop for PeerResource {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        self.connection
            .close(VarInt::from_u32(0), b"group room released");
    }
}

fn release(
    resources: &mut BTreeMap<ParticipantId, PeerResource>,
    frames: &mut Frames,
    rooms: &mut BTreeMap<String, PreparedRoom>,
    tickets: Vec<GroupParticipantTicket>,
) {
    for ticket in tickets {
        frames.remove(&ticket.id);
        if let Some(resource) = resources.remove(&ticket.id) {
            rooms.remove(&resource.key);
            drop(resource);
        }
    }
}

pub(super) async fn serve(options: ServerOptions, config: ServerConfig) -> io::Result<()> {
    let policy = GroupRoomPolicy::new(
        options.max_rooms,
        options
            .group_hosts
            .ok_or_else(|| invalid("group host policy missing"))?,
        options.max_key_bytes,
        i64::try_from(options.waiting_ttl.as_nanos()).map_err(invalid)?,
    )
    .map_err(invalid)?;
    let endpoint = Endpoint::server(config)?;
    let mut registry = GroupRoomRegistry::new(policy);
    let options = Arc::new(options);
    let permits = Arc::new(Semaphore::new(options.max_sessions));
    let mut resources = BTreeMap::<ParticipantId, PeerResource>::new();
    let mut frames = Frames::new();
    let mut rooms = BTreeMap::<String, PreparedRoom>::new();
    let handshake_timeout = i64::try_from(options.setup_timeout.as_nanos()).map_err(invalid)?;
    let mut setups = JoinSet::<io::Result<GroupPrepared>>::new();
    let mut peers = JoinSet::<(ParticipantId, io::Result<()>, Resource)>::new();
    let (commands, mut requests) = mpsc::channel::<Command>(COMMAND_CAPACITY);
    let (stop, stopped) = watch::channel(false);
    let origin = Instant::now();
    let mut expiry = tokio::time::interval(Duration::from_millis(100));
    expiry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    let result = async {
        loop {
            tokio::select! {
                biased;
                result = &mut shutdown => break result,
                _ = expiry.tick() => {
                    let time = now(origin)?;
                    release(&mut resources, &mut frames, &mut rooms, registry.expire(time).map_err(invalid)?);
                    let closed: Vec<_> = resources.iter().filter(|(_, resource)|
                        resource.connection.quic_connection().close_reason().is_some()
                        || rooms.get(&resource.key).is_some_and(|room| room.expired(time))
                    ).map(|(id, _)| *id).collect();
                    for id in closed {
                        release(&mut resources, &mut frames, &mut rooms, registry.release(id, time).map_err(invalid)?);
                    }
                }
                finished = peers.join_next(), if !peers.is_empty() => {
                    let (id, _result, resource) = finished.ok_or_else(|| invalid("peer task missing"))?.map_err(invalid)?;
                    release(&mut resources, &mut frames, &mut rooms, registry.release(id, now(origin)?).map_err(invalid)?);
                    drop(resource);
                }
                finished = setups.join_next(), if !setups.is_empty() => {
                    let candidate = finished.ok_or_else(|| invalid("setup task missing"))?.map_err(invalid)?;
                    let Ok(GroupPrepared { prepared: Prepared { key, mut resource }, identity, players }) = candidate else { continue; };
                    let time = now(origin)?;
                    release(&mut resources, &mut frames, &mut rooms, registry.expire(time).map_err(invalid)?);
                    let ticket = match registry.join(&key, &identity, &players, time) {
                        Ok(ticket) => ticket,
                        Err(_) => { drop(resource); continue; }
                    };
                    let (write, read) = resource.stream.take().ok_or_else(|| invalid("admitted stream missing"))?;
                    let (sender, receiver) = mpsc::channel(OUTGOING_CAPACITY);
                    let (peer_stop, peer_stopped) = watch::channel(false);
                    resources.insert(ticket.id, PeerResource { key, connection: resource.connection.clone(), stop: peer_stop, last_received: None });
                    frames.insert(ticket.id, sender.clone());
                    let published = (|| -> io::Result<()> {
                        let admitted = Arc::new(encode_message(&RoomMessage::Admitted { participant: ticket.id }).map_err(invalid)?);
                        sender.try_send(QueuedFrame { bytes: admitted, receipt: None }).map_err(|_| invalid("new room output closed"))?;
                        publish_room(&registry, &frames, &ticket.room)
                    })();
                    if published.is_err() {
                        release(&mut resources, &mut frames, &mut rooms, registry.release(ticket.id, time).map_err(invalid)?);
                        drop(resource);
                        continue;
                    }
                    let commands = commands.clone();
                    let limit = options.io_timeout;
                    peers.spawn(async move {
                        let result = peer_io(ticket.id, read, write, commands, receiver, limit, peer_stopped, origin).await;
                        (ticket.id, result, resource)
                    });
                }
                command = requests.recv() => {
                    let Some(command) = command else { return Err(cancelled()); };
                    let Some(peer) = resources.get(&command.id) else { continue; };
                    let key = peer.key.clone();
                    let previous_received = peer.last_received;
                    let time = now(origin)?;
                    let applied = (|| -> io::Result<Vec<GroupParticipantTicket>> {
                        if rooms.get(&key).is_some_and(|room| room.expired(time)) {
                            return Err(elapsed_timeout());
                        }
                        match command.event {
                            PeerEvent::Received(ServerControlRead { message, captured_ns }) => {
                                if captured_ns < 0 || captured_ns > time
                                    || previous_received.is_some_and(|previous| captured_ns < previous)
                                {
                                    return Err(invalid("invalid room read observation"));
                                }
                                let tickets = match &message {
                                    RoomMessage::Seal | RoomMessage::Ready | RoomMessage::Leave => {
                                        let tickets = apply_request(&mut registry, command.id, &message, time)?;
                                        if tickets.is_empty() {
                                            let snapshot = registry.room(&key).ok_or_else(|| invalid("room disappeared"))?;
                                            let prepared = if snapshot.phase == GroupRoomPhase::Prepared && !rooms.contains_key(&key) {
                                                Some(PreparedRoom::new(snapshot, time, handshake_timeout)?)
                                            } else { None };
                                            // Ordered stream queues expose Prepared before any probe/start bytes.
                                            publish_room(&registry, &frames, &key)?;
                                            if let Some(prepared) = prepared {
                                                rooms.insert(key.clone(), prepared);
                                            }
                                            if let Some(room) = rooms.get_mut(&key) {
                                                room.pump(&frames, time)?;
                                            }
                                        }
                                        tickets
                                    }
                                    _ => {
                                        let room = rooms.get_mut(&key).ok_or_else(|| invalid("room controls require Prepared membership"))?;
                                        room.receive(command.id, &message, captured_ns, time)?;
                                        room.pump(&frames, time)?;
                                        Vec::new()
                                    }
                                };
                                if let Some(peer) = resources.get_mut(&command.id) {
                                    peer.last_received = Some(captured_ns);
                                }
                                Ok(tickets)
                            }
                            PeerEvent::Written { write_id, completed_ns } => {
                                let room = rooms.get_mut(&key).ok_or_else(|| invalid("unexpected room write receipt"))?;
                                room.written(command.id, write_id, completed_ns, time)?;
                                room.pump(&frames, time)?;
                                Ok(Vec::new())
                            }
                        }
                    })();
                    let tickets = match applied {
                        Ok(tickets) => tickets,
                        Err(_) => registry.release(command.id, time).map_err(invalid)?,
                    };
                    release(&mut resources, &mut frames, &mut rooms, tickets);
                }
                incoming = endpoint.accept() => {
                    if setups.len() >= options.max_setups || peers.len() >= options.max_sessions {
                        incoming.refuse();
                    } else if let Ok(permit) = permits.clone().try_acquire_owned() {
                        let deadline = Instant::now() + options.setup_timeout;
                        setups.spawn(prepare_join(incoming, permit, options.clone(), deadline, stopped.clone()));
                    } else { incoming.refuse(); }
                }
            }
        }
    }.await;
    let _ = stop.send(true);
    endpoint.close(VarInt::from_u32(0), b"group room shutting down");
    let stopped_rooms = now(origin).and_then(|time| registry.stop(time).map_err(invalid));
    if let Ok(tickets) = &stopped_rooms {
        // Actual resources are cleared below even if ticket allocation failed.
        for ticket in tickets {
            frames.remove(&ticket.id);
            resources.remove(&ticket.id);
        }
    }
    frames.clear();
    resources.clear();
    rooms.clear();
    setups.abort_all();
    peers.abort_all();
    while setups.join_next().await.is_some() {}
    while peers.join_next().await.is_some() {}
    let _ = timeout(DRAIN, endpoint.wait_idle()).await;
    result.and(stopped_rooms.map(|_| ()))
}

#[cfg(test)]
#[path = "multiplayer_webtransport_group_fixtures.rs"]
mod fixtures;
