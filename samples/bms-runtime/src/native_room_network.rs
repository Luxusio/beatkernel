//! One native network thread around the actual common room stream driver.
//! Command admission is not a transport receipt or permission to start audio.

use crate::local_players::PlayerId;
use crate::multiplayer_group::{validate_members, GroupPrefix, MemberProgress};
use crate::multiplayer_group_rooms::{GroupRoomMember, GroupRoomPhase};
use crate::multiplayer_room_io::RoomPlayIo;
use crate::multiplayer_room_play::RoomPlayClient;
use crate::multiplayer_rooms::ParticipantId;
use crate::multiplayer_start::{StartPolicy, StartSchedule};
use crate::multiplayer_webtransport_client::WebTransportOptions;
use std::{
    collections::VecDeque,
    fmt,
    io::{self, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
        Arc, Mutex, TryLockError,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const IDLE: Duration = Duration::from_millis(1);
const MAX_TIMEOUT: Duration = Duration::from_secs(120);

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

/// Nonblocking/deadline-bounded stream operations run only on the network thread.
/// `finish` is attempted on success, cancellation and failure, before dropping it.
pub trait NativeRoomStream: Read + Write {
    fn idle(&mut self, duration: Duration) -> io::Result<()>;
    fn finish(&mut self, timeout: Duration) -> io::Result<()>;
}

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

/// Fixed deadlines include connection through commitment, and explicit drain
/// admission through genuine Complete respectively. They are never renewed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeRoomOptions {
    pub setup_timeout: Duration,
    pub drain_timeout: Duration,
    pub finish_timeout: Duration,
    pub queue_capacity: usize,
    pub start_policy: StartPolicy,
    pub preroll_ns: i64,
}
impl Default for NativeRoomOptions {
    fn default() -> Self {
        Self {
            setup_timeout: Duration::from_secs(60),
            drain_timeout: Duration::from_secs(10),
            finish_timeout: Duration::from_secs(2),
            queue_capacity: 32,
            start_policy: StartPolicy::default(),
            preroll_ns: 0,
        }
    }
}
impl NativeRoomOptions {
    fn validate(self) -> io::Result<()> {
        if !(1..=1024).contains(&self.queue_capacity)
            || [self.setup_timeout, self.drain_timeout, self.finish_timeout]
                .iter()
                .any(|limit| *limit < Duration::from_millis(1) || *limit > MAX_TIMEOUT)
            || self.preroll_ns < 0
        {
            return Err(invalid("invalid bounded native room options"));
        }
        self.start_policy
            .validate()
            .map_err(|error| invalid(error.to_string()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeRoomCommand {
    Seal,
    Ready,
    Leave,
    Publish {
        members: Vec<MemberProgress>,
        final_prefix: bool,
    },
    /// Admit one fixed deadline; queue common DrainReady only after actual local completion.
    Drain,
}

/// Retained errors remain readable after stream and thread ownership have joined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeRoomFailure {
    pub kind: io::ErrorKind,
    pub message: String,
}
impl From<io::Error> for NativeRoomFailure {
    fn from(error: io::Error) -> Self {
        Self {
            kind: error.kind(),
            message: error.to_string(),
        }
    }
}
impl fmt::Display for NativeRoomFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for NativeRoomFailure {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeRoomReply {
    pub id: u64,
    pub result: Result<(), NativeRoomFailure>,
}
pub use crate::room_final_wait::RoomFinalReceipts as NativeRoomReceipts;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeRoomRoster {
    pub members: Vec<GroupRoomMember>,
    pub phase: GroupRoomPhase,
    pub deadline_ns: Option<i64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeRoomOutcome {
    pub cancelled: bool,
    pub error: Option<NativeRoomFailure>,
    pub cleanup_error: Option<NativeRoomFailure>,
    pub receipts: NativeRoomReceipts,
    pub leave_written: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeRoomSnapshot {
    /// Changes only when an actual accepted room snapshot changes.
    pub revision: u64,
    pub participant: Option<ParticipantId>,
    pub room: Option<Arc<NativeRoomRoster>>,
    /// Retained genuine schedule on this owner's elapsed clock, never a readiness guess.
    pub schedule: Option<StartSchedule>,
    /// Exact host-qualified prefixes in frozen room order, copied only on sequence changes.
    pub peers: Vec<(ParticipantId, Arc<GroupPrefix>)>,
    pub receipts: NativeRoomReceipts,
    pub terminal: Option<NativeRoomOutcome>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeRoomPoll {
    pub snapshot: NativeRoomSnapshot,
    pub replies: Vec<NativeRoomReply>,
}

struct Drain {
    deadline_ns: i64,
    requested: bool,
}

/// The same bounded actor is used by the thread and in-memory fixture streams.
struct Actor<S: NativeRoomStream> {
    io: RoomPlayIo<S>,
    options: NativeRoomOptions,
    snapshot: NativeRoomSnapshot,
    last_now: Option<i64>,
    drain: Option<Drain>,
    leaving: bool,
    changed: bool,
}
impl<S: NativeRoomStream> Actor<S> {
    fn new(io: RoomPlayIo<S>, options: NativeRoomOptions) -> io::Result<Self> {
        // The public owner validates options before acquiring a stream.
        options.validate()?;
        Ok(Self {
            io,
            options,
            snapshot: NativeRoomSnapshot::default(),
            last_now: None,
            drain: None,
            leaving: false,
            changed: false,
        })
    }

    fn snapshot(&self) -> &NativeRoomSnapshot {
        &self.snapshot
    }

    fn check_now(&self, now: i64) -> io::Result<()> {
        if now < 0 || self.last_now.is_some_and(|previous| now < previous) {
            return Err(invalid("native room observation is negative or regressing"));
        }
        Ok(())
    }

    fn command(&mut self, command: NativeRoomCommand, now: i64) -> io::Result<()> {
        self.check_now(now)?;
        if self.finished() || self.leaving {
            return Err(invalid("native room no longer accepts commands"));
        }
        match command {
            NativeRoomCommand::Seal => self.io.request_seal()?,
            NativeRoomCommand::Ready => self.io.request_ready()?,
            NativeRoomCommand::Leave => {
                self.io.request_leave()?;
                self.leaving = true;
            }
            NativeRoomCommand::Publish {
                members,
                final_prefix,
            } => {
                self.io.publish_progress(&members, final_prefix)?;
            }
            NativeRoomCommand::Drain => {
                if self.snapshot.schedule.is_none() || self.drain.is_some() {
                    return Err(invalid(
                        "native room drain requires one committed live owner",
                    ));
                }
                let deadline_ns = now
                    .checked_add(nanos(self.options.drain_timeout)?)
                    .ok_or_else(|| invalid("native room drain deadline overflow"))?;
                let requested = self.io.progress_complete();
                if requested {
                    self.io.request_drain()?;
                }
                self.drain = Some(Drain {
                    deadline_ns,
                    requested,
                });
            }
        }
        self.last_now = Some(now);
        Ok(())
    }

    fn refresh(&mut self) -> io::Result<()> {
        let participant = self.io.session().participant();
        if self.snapshot.participant != participant {
            self.snapshot.participant = participant;
            self.changed = true;
        }
        if let Some(room) = self.io.session().room() {
            let changed = self.snapshot.room.as_ref().is_none_or(|old| {
                old.phase != room.phase
                    || old.deadline_ns != room.deadline_ns
                    || old.members.as_slice() != room.members
            });
            if changed {
                let revision = self
                    .snapshot
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| invalid("native room revision exhausted"))?;
                let mut members = Vec::new();
                members
                    .try_reserve_exact(room.members.len())
                    .map_err(allocation)?;
                for member in room.members {
                    members.push(GroupRoomMember {
                        id: member.id,
                        players: copy_slice(&member.players)?,
                        prepared: member.prepared,
                    });
                }
                self.snapshot.room = Some(Arc::new(NativeRoomRoster {
                    members,
                    phase: room.phase,
                    deadline_ns: room.deadline_ns,
                }));
                self.snapshot.revision = revision;
                self.changed = true;
            }
            for member in room.members {
                if let Some(prefix) = self.io.peer_progress(member.id) {
                    let old = self
                        .snapshot
                        .peers
                        .iter()
                        .position(|(id, _)| *id == member.id);
                    if old.is_none_or(|index| {
                        self.snapshot.peers[index].1.sequence != prefix.sequence
                    }) {
                        let next = Arc::new(GroupPrefix {
                            sequence: prefix.sequence,
                            final_prefix: prefix.final_prefix,
                            members: copy_slice(&prefix.members)?,
                        });
                        if let Some(index) = old {
                            self.snapshot.peers[index].1 = next;
                        } else {
                            self.snapshot.peers.try_reserve(1).map_err(allocation)?;
                            self.snapshot.peers.push((member.id, next));
                            // Different peers can publish first in any order; presentation stays roster-ordered.
                            self.snapshot.peers.sort_by_key(|(id, _)| {
                                room.members.iter().position(|member| member.id == *id)
                            });
                        }
                        self.changed = true;
                    }
                }
            }
        }
        let receipts = NativeRoomReceipts {
            local_final_written: self.snapshot.receipts.local_final_written
                || self.io.local_final_written(),
            local_final_acknowledged: self.snapshot.receipts.local_final_acknowledged
                || self.io.local_final_acknowledged(),
            progress_complete: self.snapshot.receipts.progress_complete
                || self.io.progress_complete(),
            drain_complete: self.snapshot.receipts.drain_complete || self.io.drain_complete(),
        };
        if self.snapshot.receipts != receipts {
            self.snapshot.receipts = receipts;
            self.changed = true;
        }
        if self.snapshot.schedule.is_none() && !self.io.session().leave_written() {
            if let Some(schedule) = self.io.take_schedule()? {
                self.snapshot.schedule = Some(schedule);
                self.changed = true;
            }
        }
        Ok(())
    }

    fn finished(&self) -> bool {
        self.io.session().leave_written() || self.io.drain_complete()
    }

    fn drive<F: FnMut() -> io::Result<i64>>(&mut self, mut now: F) -> io::Result<bool> {
        if self.finished() {
            return Ok(false);
        }
        let observed = now()?;
        self.check_now(observed)?;
        self.last_now = Some(observed);
        self.refresh()?;
        if self.snapshot.schedule.is_none() && observed >= nanos(self.options.setup_timeout)? {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "native room setup deadline expired",
            ));
        }
        if let Some(drain) = &mut self.drain {
            if observed >= drain.deadline_ns {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "native room drain deadline expired",
                ));
            }
            if !drain.requested && self.io.progress_complete() {
                self.io.request_drain()?;
                drain.requested = true;
            }
        }
        let setup_pending = self.snapshot.schedule.is_none();
        let last = &mut self.last_now;
        let result = self.io.step(|| {
            let value = now()?;
            if value < 0 || last.is_some_and(|previous| value < previous) {
                return Err(invalid("native room clock is negative or regressing"));
            }
            *last = Some(value);
            Ok(value)
        });
        // Preserve any accepted prefix even when a later I/O operation failed.
        let refreshed = self.refresh();
        match result {
            Err(error) => Err(error),
            Ok(progressed) => {
                refreshed?;
                // A bounded transport operation can complete across its fixed
                // deadline. Real late receipts remain history, never timely success.
                let completed = self.last_now.unwrap_or(observed);
                if setup_pending && completed >= nanos(self.options.setup_timeout)? {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "native room setup deadline expired during I/O",
                    ));
                }
                if self
                    .drain
                    .as_ref()
                    .is_some_and(|drain| completed >= drain.deadline_ns)
                {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "native room drain deadline expired during I/O",
                    ));
                }
                Ok(progressed)
            }
        }
    }

    fn finish(mut self, error: Option<NativeRoomFailure>, cancelled: bool) -> NativeRoomOutcome {
        let leave_written = self.io.session().leave_written();
        let refresh_error = self.refresh().err().map(NativeRoomFailure::from);
        let receipts = self.snapshot.receipts;
        self.io.stop();
        let mut stream = self.io.into_stream();
        let cleanup_error = stream
            .finish(self.options.finish_timeout)
            .err()
            .map(NativeRoomFailure::from);
        NativeRoomOutcome {
            cancelled,
            error: error.or(refresh_error),
            cleanup_error,
            receipts,
            leave_written,
        }
    }
}

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
    if actor.changed {
        shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot = actor.snapshot().clone();
        actor.changed = false;
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
                    if let Err(failure) = actor.io.stream_mut().idle(IDLE) {
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
    let cancelled = stop.load(Ordering::Acquire) || actor.io.session().leave_written();
    let outcome = actor.finish(error, cancelled);
    terminal(shared, &receiver, outcome);
}

#[cfg(test)]
#[path = "native_room_network_fixtures.rs"]
mod fixtures;
