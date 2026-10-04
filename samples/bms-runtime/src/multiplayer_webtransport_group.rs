//! Actual BKMR admission owner. No gameplay/start/progress traffic is accepted.

use super::*;
use crate::local_players::PlayerId;
use crate::multiplayer_group_rooms::{
    GroupParticipantTicket, GroupRoomMember, GroupRoomPolicy, GroupRoomRegistry,
};
use crate::multiplayer_room_wire::{RoomFrameDecoder, RoomMessage, encode_message};
use tokio::sync::mpsc;

const OUTGOING_CAPACITY: usize = 4;
const COMMAND_CAPACITY: usize = 256;
type Frames = BTreeMap<ParticipantId, mpsc::Sender<Arc<Vec<u8>>>>;

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

/// Frames originate from the actual room codec. Each queue is bounded, and a
/// completed write is only local transport progress, never a room/gameplay ACK.
async fn write_frames<W: AsyncWrite + Unpin>(
    mut write: W,
    mut frames: mpsc::Receiver<Arc<Vec<u8>>>,
    limit: Duration,
    mut stop: watch::Receiver<bool>,
) -> io::Result<()> {
    if limit.is_zero() || limit > Duration::from_secs(120) {
        return Err(invalid("invalid room I/O timeout"));
    }
    tokio::select! {
        biased;
        _ = wait_stop(&mut stop) => Err(cancelled()),
        result = async {
            while let Some(frame) = frames.recv().await {
                timeout(limit, write.write_all(frame.as_slice())).await.map_err(|_| elapsed_timeout())??;
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
        slot.send(frame.clone());
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

struct Command {
    id: ParticipantId,
    message: RoomMessage,
}

/// One joined task owns both stream futures and their original Resource. The
/// resource is returned on completion so its session permit survives until join.
async fn peer_io(
    id: ParticipantId,
    mut read: RecvStream,
    write: SendStream,
    commands: mpsc::Sender<Command>,
    frames: mpsc::Receiver<Arc<Vec<u8>>>,
    limit: Duration,
    mut stop: watch::Receiver<bool>,
) -> io::Result<()> {
    let mut reading_stop = stop.clone();
    let writing_stop = stop.clone();
    let reader = async {
        while let Some(message) = read_message(&mut read, limit, &mut reading_stop).await? {
            if !matches!(
                message,
                RoomMessage::Seal | RoomMessage::Ready | RoomMessage::Leave
            ) {
                return Err(invalid("unexpected admitted room message"));
            }
            commands
                .send(Command { id, message })
                .await
                .map_err(|_| cancelled())?;
        }
        Ok(())
    };
    let writer = write_frames(write, frames, limit, writing_stop);
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
    tickets: Vec<GroupParticipantTicket>,
) {
    for ticket in tickets {
        frames.remove(&ticket.id);
        resources.remove(&ticket.id);
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
                finished = peers.join_next(), if !peers.is_empty() => {
                    let (id, _result, resource) = finished.ok_or_else(|| invalid("peer task missing"))?.map_err(invalid)?;
                    release(&mut resources, &mut frames, registry.release(id, now(origin)?).map_err(invalid)?);
                    drop(resource);
                }
                finished = setups.join_next(), if !setups.is_empty() => {
                    let candidate = finished.ok_or_else(|| invalid("setup task missing"))?.map_err(invalid)?;
                    let Ok(GroupPrepared { prepared: Prepared { key, mut resource }, identity, players }) = candidate else { continue; };
                    let time = now(origin)?;
                    release(&mut resources, &mut frames, registry.expire(time).map_err(invalid)?);
                    let ticket = match registry.join(&key, &identity, &players, time) {
                        Ok(ticket) => ticket,
                        Err(_) => { drop(resource); continue; }
                    };
                    let (write, read) = resource.stream.take().ok_or_else(|| invalid("admitted stream missing"))?;
                    let (sender, receiver) = mpsc::channel(OUTGOING_CAPACITY);
                    let (peer_stop, peer_stopped) = watch::channel(false);
                    resources.insert(ticket.id, PeerResource { key, connection: resource.connection.clone(), stop: peer_stop });
                    frames.insert(ticket.id, sender.clone());
                    let published = (|| -> io::Result<()> {
                        let admitted = Arc::new(encode_message(&RoomMessage::Admitted { participant: ticket.id }).map_err(invalid)?);
                        sender.try_send(admitted).map_err(|_| invalid("new room output closed"))?;
                        publish_room(&registry, &frames, &ticket.room)
                    })();
                    if published.is_err() {
                        release(&mut resources, &mut frames, registry.release(ticket.id, time).map_err(invalid)?);
                        drop(resource);
                        continue;
                    }
                    let commands = commands.clone();
                    let limit = options.io_timeout;
                    peers.spawn(async move {
                        let result = peer_io(ticket.id, read, write, commands, receiver, limit, peer_stopped).await;
                        (ticket.id, result, resource)
                    });
                }
                command = requests.recv() => {
                    let Some(command) = command else { return Err(cancelled()); };
                    let Some(peer) = resources.get(&command.id) else { continue; };
                    let time = now(origin)?;
                    let applied = apply_request(&mut registry, command.id, &command.message, time)
                        .and_then(|tickets| {
                            if tickets.is_empty() { publish_room(&registry, &frames, &peer.key)?; }
                            Ok(tickets)
                        });
                    let tickets = match applied {
                        Ok(tickets) => tickets,
                        Err(_) => registry.release(command.id, time).map_err(invalid)?,
                    };
                    release(&mut resources, &mut frames, tickets);
                }
                _ = expiry.tick() => {
                    let time = now(origin)?;
                    release(&mut resources, &mut frames, registry.expire(time).map_err(invalid)?);
                    let closed: Vec<_> = resources.iter().filter(|(_, resource)| resource.connection.quic_connection().close_reason().is_some()).map(|(id, _)| *id).collect();
                    for id in closed {
                        release(&mut resources, &mut frames, registry.release(id, time).map_err(invalid)?);
                    }
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
    endpoint.close(VarInt::from_u32(0), b"group admission shutting down");
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
