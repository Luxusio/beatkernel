//! One native network thread around the actual common room stream driver.
//! Command admission is not a transport receipt or permission to start audio.

use crate::local_players::PlayerId;
use crate::multiplayer_group::validate_members;
use crate::multiplayer_room_io::RoomPlayIo;
use crate::multiplayer_room_play::RoomPlayClient;
#[cfg(test)]
use crate::multiplayer_rooms::ParticipantId;
use crate::multiplayer_webtransport_client::WebTransportOptions;
use std::{
    collections::VecDeque,
    fmt, io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
        Arc, Mutex, TryLockError,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const IDLE: Duration = Duration::from_millis(1);

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
fn allocation(error: impl fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
fn nanos(duration: Duration) -> io::Result<i64> {
    i64::try_from(duration.as_nanos()).map_err(|_| invalid("room clock exceeds signed nanoseconds"))
}
fn elapsed(origin: Instant) -> io::Result<i64> {
    nanos(origin.elapsed())
}
fn copy_slice<T: Clone>(slice: &[T]) -> io::Result<Vec<T>> {
    let mut result = Vec::new();
    result.try_reserve_exact(slice.len()).map_err(allocation)?;
    result.extend_from_slice(slice);
    Ok(result)
}

pub use crate::room_network_actor::RoomNetworkStream as NativeRoomStream;
use crate::room_network_actor::RoomNetworkActor as Actor;

#[cfg(feature = "webtransport")]
impl NativeRoomStream for crate::multiplayer_webtransport_client::WebTransportStream {
    fn idle(&mut self, duration: Duration) -> io::Result<()> {
        crate::multiplayer_webtransport_client::WebTransportStream::idle(self, duration);
        Ok(())
    }
    fn finish(&mut self, timeout: Duration) -> io::Result<()> {
        crate::multiplayer_webtransport_client::WebTransportStream::finish(self, timeout)
    }
}

pub use crate::room_network_model::{
    RoomNetworkOptions as NativeRoomOptions, RoomCommand as NativeRoomCommand,
    RoomFailure as NativeRoomFailure, RoomReply as NativeRoomReply,
    RoomReceipts as NativeRoomReceipts, RoomRoster as NativeRoomRoster,
    RoomOutcome as NativeRoomOutcome, RoomSnapshot as NativeRoomSnapshot,
    RoomPoll as NativeRoomPoll,
};

struct Envelope {
    id: u64,
    command: NativeRoomCommand,
}
struct Shared {
    snapshot: NativeRoomSnapshot,
    replies: VecDeque<NativeRoomReply>,
    outstanding: usize,
}

/// A nonblocking front end. Call `stop` and inspect its outcome before replacing
/// an owner; Drop also cancels and joins, but cannot return a cleanup error.
pub struct NativeRoomNetwork {
    origin: Instant,
    stop: Arc<AtomicBool>,
    commands: SyncSender<Envelope>,
    shared: Arc<Mutex<Shared>>,
    worker: Option<JoinHandle<()>>,
    next_id: Option<u64>,
    players: Vec<PlayerId>,
    capacity: usize,
}
impl NativeRoomNetwork {
    /// Connector runs on the dedicated thread and receives a fresh validated
    /// common client, cancellation flag and the original absolute setup deadline.
    /// It must return this client's actual stream owner with no removed frame.
    pub fn spawn_with<S, C>(
        identity: &[u8],
        players: &[PlayerId],
        options: NativeRoomOptions,
        connector: C,
    ) -> io::Result<Self>
    where
        S: NativeRoomStream + 'static,
        C: FnOnce(RoomPlayClient, &AtomicBool, Instant) -> io::Result<RoomPlayIo<S>>
            + Send
            + 'static,
    {
        options.validate()?;
        let session =
            RoomPlayClient::new(identity, players, options.start_policy, options.preroll_ns)
                .map_err(|error| invalid(error.to_string()))?;
        let players = copy_slice(players)?;
        let mut replies = VecDeque::new();
        replies
            .try_reserve_exact(options.queue_capacity)
            .map_err(allocation)?;
        let shared = Arc::new(Mutex::new(Shared {
            snapshot: NativeRoomSnapshot::default(),
            replies,
            outstanding: 0,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (commands, receiver) = mpsc::sync_channel(options.queue_capacity);
        let origin = Instant::now();
        let deadline = origin
            .checked_add(options.setup_timeout)
            .ok_or_else(|| invalid("native room setup deadline overflow"))?;
        let worker_shared = shared.clone();
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("bms-room-network".into())
            .spawn(move || {
                let acquired = if worker_stop.load(Ordering::Acquire) {
                    Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "native room acquisition cancelled",
                    ))
                } else {
                    connector(session, &worker_stop, deadline)
                };
                match acquired {
                    Ok(io) => {
                        // Options were admitted before any thread or stream acquisition.
                        match Actor::new(io, options) {
                            Ok(actor) => run(actor, receiver, &worker_shared, &worker_stop, origin),
                            Err(error) => terminal(
                                &worker_shared,
                                &receiver,
                                NativeRoomOutcome {
                                    cancelled: worker_stop.load(Ordering::Acquire),
                                    error: Some(error.into()),
                                    cleanup_error: None,
                                    receipts: NativeRoomReceipts::default(),
                                    leave_written: false,
                                },
                            ),
                        }
                    }
                    Err(error) => terminal(
                        &worker_shared,
                        &receiver,
                        NativeRoomOutcome {
                            cancelled: worker_stop.load(Ordering::Acquire),
                            error: Some(error.into()),
                            cleanup_error: None,
                            receipts: NativeRoomReceipts::default(),
                            leave_written: false,
                        },
                    ),
                }
            })?;
        Ok(Self {
            origin,
            stop,
            commands,
            shared,
            worker: Some(worker),
            next_id: Some(1),
            players,
            capacity: options.queue_capacity,
        })
    }

    /// Actual trusted WebTransport preparation and connection both occur on the
    /// network thread. Its bilateral role option does not choose room authority.
    pub fn webtransport(
        transport: WebTransportOptions,
        identity: &[u8],
        players: &[PlayerId],
        options: NativeRoomOptions,
    ) -> io::Result<Self> {
        #[cfg(feature = "webtransport")]
        {
            transport.validate()?;
            // The public spawn preflight precedes all credential/socket I/O;
            // connect_room_play then creates the actual session at that endpoint.
            if identity.len() > crate::multiplayer_protocol::MAX_IDENTITY || players.len() > 64 {
                return Err(invalid("native room identity or roster exceeds its bound"));
            }
            let owned_identity = copy_slice(identity)?;
            let owned_players = copy_slice(players)?;
            Self::spawn_with(
                identity,
                players,
                options,
                move |_validated, stop, deadline| {
                    let endpoint =
                        crate::multiplayer_webtransport_client::WebTransportEndpoint::prepare(
                            &transport,
                        )?;
                    endpoint.connect_room_play(
                        &owned_identity,
                        &owned_players,
                        options.start_policy,
                        options.preroll_ns,
                        stop,
                        deadline,
                    )
                },
            )
        }
        #[cfg(not(feature = "webtransport"))]
        {
            let _ = (transport, identity, players, options);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "native room WebTransport feature is disabled",
            ))
        }
    }

    /// Same origin used by every actual stream observation, suitable for a
    /// caller's genuine native host-clock bracket. No output clock is inferred.
    pub fn clock_now_ns(&self) -> io::Result<i64> {
        elapsed(self.origin)
    }

    /// The bound covers queued, processing and unread replies together. Busy
    /// rejection consumes neither a command identity nor an outstanding slot.
    pub fn try_command(&mut self, command: NativeRoomCommand) -> io::Result<u64> {
        if self.stop.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "native room is stopping",
            ));
        }
        if let NativeRoomCommand::Publish { members, .. } = &command {
            validate_members(None, members).map_err(|error| invalid(error.to_string()))?;
            if members.len() != self.players.len()
                || members
                    .iter()
                    .zip(&self.players)
                    .any(|(member, player)| member.player != *player)
            {
                return Err(invalid(
                    "native room publication changed the ordered local roster",
                ));
            }
        }
        let mut shared = self.shared.try_lock().map_err(lock_error)?;
        if shared.snapshot.terminal.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "native room has terminated",
            ));
        }
        if shared.outstanding >= self.capacity {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "native room command replies are full",
            ));
        }
        let id = self
            .next_id
            .ok_or_else(|| invalid("native room command identity exhausted"))?;
        self.commands
            .try_send(Envelope { id, command })
            .map_err(|error| match error {
                TrySendError::Full(_) => io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "native room command queue is full",
                ),
                TrySendError::Disconnected(_) => {
                    io::Error::new(io::ErrorKind::NotConnected, "native room worker is closed")
                }
            })?;
        self.next_id = id.checked_add(1);
        shared.outstanding += 1;
        Ok(id)
    }

    /// Cheap Arc copies retain independently readable room/prefix snapshots.
    /// Only polling releases reply credits; no unbounded event queue exists.
    pub fn poll(&self) -> io::Result<NativeRoomPoll> {
        let mut shared = self.shared.try_lock().map_err(lock_error)?;
        let snapshot = shared.snapshot.clone();
        let replies: Vec<_> = shared.replies.drain(..).collect();
        shared.outstanding -= replies.len();
        Ok(NativeRoomPoll { snapshot, replies })
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    /// Join acquisition, all I/O and actual finish before returning. A failed
    /// network operation and a failed cleanup remain distinct retained fields.
    pub fn stop(&mut self) -> NativeRoomOutcome {
        self.request_stop();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                let mut shared = self
                    .shared
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let failure = NativeRoomFailure {
                    kind: io::ErrorKind::Other,
                    message: "native room network thread panicked while joining".into(),
                };
                if let Some(outcome) = &mut shared.snapshot.terminal {
                    outcome.cleanup_error.get_or_insert(failure);
                } else {
                    shared.snapshot.terminal = Some(NativeRoomOutcome {
                        cancelled: true,
                        error: Some(failure),
                        cleanup_error: None,
                        receipts: shared.snapshot.receipts,
                        leave_written: false,
                    });
                }
            }
        }
        let shared = self
            .shared
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        shared
            .snapshot
            .terminal
            .clone()
            .unwrap_or_else(|| NativeRoomOutcome {
                cancelled: true,
                error: Some(NativeRoomFailure {
                    kind: io::ErrorKind::Other,
                    message: "native room worker exited without an outcome".into(),
                }),
                cleanup_error: None,
                receipts: shared.snapshot.receipts,
                leave_written: false,
            })
    }
}
impl Drop for NativeRoomNetwork {
    fn drop(&mut self) {
        if self.worker.is_some() {
            let outcome = self.stop();
            if let Some(error) = outcome.cleanup_error.as_ref().or(outcome.error.as_ref()) {
                eprintln!("native room owner dropped after failure: {error}");
            }
        }
    }
}

fn lock_error<T>(error: TryLockError<T>) -> io::Error {
    match error {
        TryLockError::WouldBlock => {
            io::Error::new(io::ErrorKind::WouldBlock, "native room snapshot is busy")
        }
        TryLockError::Poisoned(_) => io::Error::other("native room shared state is poisoned"),
    }
}
fn publish<S: NativeRoomStream>(actor: &mut Actor<S>, shared: &Mutex<Shared>) {
    if let Some(snapshot) = actor.take_changed_snapshot() {
        shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot = snapshot.clone();
    }
}
fn terminal(shared: &Mutex<Shared>, receiver: &Receiver<Envelope>, outcome: NativeRoomOutcome) {
    let mut shared = shared.lock().unwrap_or_else(|error| error.into_inner());
    shared.snapshot.receipts = outcome.receipts;
    shared.snapshot.terminal = Some(outcome);
    // Admission holds this same lock, so no command can escape terminal settlement.
    while let Ok(command) = receiver.try_recv() {
        shared.replies.push_back(NativeRoomReply {
            id: command.id,
            result: Err(NativeRoomFailure {
                kind: io::ErrorKind::NotConnected,
                message: "native room terminated before this command".into(),
            }),
        });
    }
}
fn run<S: NativeRoomStream>(
    mut actor: Actor<S>,
    receiver: Receiver<Envelope>,
    shared: &Mutex<Shared>,
    stop: &AtomicBool,
    origin: Instant,
) {
    let mut error = None;
    while !stop.load(Ordering::Acquire) && !actor.finished() {
        match receiver.try_recv() {
            Ok(command) => {
                let result = elapsed(origin).and_then(|now| actor.command(command.command, now));
                shared
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .replies
                    .push_back(NativeRoomReply {
                        id: command.id,
                        result: result.map_err(NativeRoomFailure::from),
                    });
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => break,
        }
        match actor.drive(|| elapsed(origin)) {
            Ok(progressed) => {
                publish(&mut actor, shared);
                if !progressed && !actor.finished() && !stop.load(Ordering::Acquire) {
                    if let Err(failure) = actor.idle(IDLE) {
                        error = Some(failure.into());
                        break;
                    }
                }
            }
            Err(failure) => {
                error = Some(failure.into());
                break;
            }
        }
    }
    publish(&mut actor, shared);
    let cancelled = stop.load(Ordering::Acquire) || actor.leave_written();
    let outcome = actor.finish(error, cancelled);
    terminal(shared, &receiver, outcome);
}

#[cfg(test)]
#[path = "native_room_network_fixtures.rs"]
mod fixtures;

impl crate::room_network_model::RoomNetworkPort for NativeRoomNetwork {
    fn try_command(&mut self, command: NativeRoomCommand) -> io::Result<u64> {
        NativeRoomNetwork::try_command(self, command)
    }
    fn poll(&self) -> io::Result<NativeRoomPoll> {
        NativeRoomNetwork::poll(self)
    }
    fn clock_now_ns(&self) -> io::Result<i64> {
        NativeRoomNetwork::clock_now_ns(self)
    }
    fn request_stop(&self) {
        NativeRoomNetwork::request_stop(self);
    }
    fn stop(&mut self) -> NativeRoomOutcome {
        NativeRoomNetwork::stop(self)
    }
}
