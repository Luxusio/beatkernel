//! Casual two-peer progress transport with scalar or whole-cohort prefixes.
//! Remote scores are unauthenticated display data.
//! Socket work never runs on the gameplay or audio thread. Software starts are committed.
use crate::multiplayer_clock::OffsetEstimate;
use crate::multiplayer_configuration::{validate_identity, validate_options};
use crate::local_players::PlayerId;
use crate::multiplayer_group::{GroupPrefix, MemberProgress, validate_members};
#[cfg(test)]
use crate::multiplayer_clock::{ClockFilter, ClockSample};
use crate::multiplayer_protocol::{
    FrameDecoder as Frames, MAX_IDENTITY, OutboundFrame, Outgoing, Session, WriteStep,
    validate_progress,
};
pub use crate::multiplayer_protocol::{GroupEvent, MultiplayerError, MultiplayerEvent, Progress};
#[cfg(test)]
use crate::multiplayer_protocol::{
    ClockProbes, Protocol, VERSION, frame, parse_progress, parse_start_frame, prefix_frame,
    progress_frame, start_frame,
};
use crate::multiplayer_quic::{QuicEndpoint, QuicStream};
use crate::multiplayer_webtransport_client::WebTransportOptions;
#[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
use crate::multiplayer_webtransport_client::{WebTransportEndpoint, WebTransportStream};
#[cfg(test)]
use crate::multiplayer_start::StartMessage;
use crate::multiplayer_start::{StartPolicy, StartRole, StartSchedule};
use beatkernel::{
    replay::{
        ReplayHeader,
        codec::{ReplayCodecLimits, ReplayFile, encode_replay},
    },
    time::{ClockDomainId, Timestamp},
};
use std::{
    io::{self, Read, Write},
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const TICK: Duration = Duration::from_millis(5);

// Transport ownership varies; framing and every peer transition remain in the
// same Session and network-worker loop below.
enum Endpoint {
    Quic(QuicEndpoint),
    #[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
    WebTransport(WebTransportEndpoint),
}
impl Endpoint {
    fn connect(self, stop: &AtomicBool, deadline: Instant) -> io::Result<Stream> {
        match self {
            Self::Quic(endpoint) => endpoint.connect(stop, deadline).map(Stream::Quic),
            #[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
            Self::WebTransport(endpoint) => {
                endpoint.connect(stop, deadline).map(Stream::WebTransport)
            }
        }
    }
}
enum Stream {
    Quic(QuicStream),
    #[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
    WebTransport(WebTransportStream),
}
impl Stream {
    fn idle(&self, duration: Duration) {
        match self {
            Self::Quic(stream) => stream.idle(duration),
            #[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
            Self::WebTransport(stream) => stream.idle(duration),
        }
    }
    fn finish(&mut self, timeout: Duration) -> io::Result<()> {
        match self {
            Self::Quic(stream) => stream.finish(timeout),
            #[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
            Self::WebTransport(stream) => stream.finish(timeout),
        }
    }
}
impl Read for Stream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Quic(stream) => stream.read(buffer),
            #[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
            Self::WebTransport(stream) => stream.read(buffer),
        }
    }
}
impl Write for Stream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self {
            Self::Quic(stream) => stream.write(buffer),
            #[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
            Self::WebTransport(stream) => stream.write(buffer),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Quic(stream) => stream.flush(),
            #[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
            Self::WebTransport(stream) => stream.flush(),
        }
    }
}

/// Exact chart/rules/options/seed/runtime compatibility, independent of capture clock.
pub fn competition_identity(
    header: &ReplayHeader,
    runtime_version: &str,
    limits: ReplayCodecLimits,
) -> Result<Vec<u8>, MultiplayerError> {
    let mut header = header.clone();
    header.normalized_clock = ClockDomainId(0);
    let mut file = ReplayFile::new(header, Vec::new());
    file.runtime_version = runtime_version.into();
    let identity = encode_replay(&file, limits)
        .map_err(|error| MultiplayerError::Protocol(error.to_string()))?;
    validate_identity(&identity)?;
    Ok(identity)
}

/// Exact finite-section agreement around the unchanged normalized setup identity.
/// `None` preserves legacy bytes. Endpoints remain outside captured replay headers.
pub fn competition_identity_for_section(
    header: &ReplayHeader,
    runtime_version: &str,
    limits: ReplayCodecLimits,
    end: Option<Timestamp>,
) -> Result<Vec<u8>, MultiplayerError> {
    let Some(end) = end else {
        return competition_identity(header, runtime_version, limits);
    };
    // Bound caller-assembled metadata through the canonical codec before the
    // profile decoder allocates its window vector.
    let legacy = competition_identity(header, runtime_version, limits)?;
    let setup = crate::replay_playback::decode_section_setup(&header.options)
        .map_err(|error| MultiplayerError::Protocol(error.to_string()))?;
    if setup.end.is_some() {
        return Err(MultiplayerError::Protocol(
            "section endpoint must be carried only by the competition envelope".into(),
        ));
    }
    let start = setup.start;
    if start.as_nanos() < 0 || end.as_nanos() < 0 || end <= start {
        return Err(MultiplayerError::Protocol(
            "section endpoint must be nonnegative and after start".into(),
        ));
    }
    let prefix = b"bms-competition-section/v1:";
    let length = prefix
        .len()
        .checked_add(8)
        .and_then(|n| n.checked_add(legacy.len()))
        .filter(|n| *n <= MAX_IDENTITY)
        .ok_or_else(|| MultiplayerError::Protocol("section identity exceeds limit".into()))?;
    let mut identity = Vec::new();
    identity
        .try_reserve_exact(length)
        .map_err(|_| MultiplayerError::Protocol("section identity allocation failed".into()))?;
    identity.extend_from_slice(prefix);
    identity.extend_from_slice(&end.as_nanos().to_le_bytes());
    identity.extend_from_slice(&legacy);
    validate_identity(&identity)?;
    Ok(identity)
}

pub use crate::multiplayer_configuration::MultiplayerOptions;

impl From<io::Error> for MultiplayerError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Lifecycle and whole-cohort observations from the same bounded worker queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MultiplayerNotice {
    Session(MultiplayerEvent),
    Group(GroupEvent),
}

enum OutgoingMessage {
    Scalar(Outgoing),
    Group {
        members: Vec<MemberProgress>,
        final_prefix: bool,
    },
}
impl OutgoingMessage {
    fn final_prefix(&self) -> bool {
        match self {
            Self::Scalar(message) => message.final_prefix,
            Self::Group { final_prefix, .. } => *final_prefix,
        }
    }
    fn try_clone(&self) -> Result<Self, MultiplayerError> {
        Ok(match self {
            Self::Scalar(message) => Self::Scalar(Outgoing {
                progress: message.progress,
                final_prefix: message.final_prefix,
            }),
            Self::Group {
                members,
                final_prefix,
            } => Self::Group {
                members: copy_group_values(members)?,
                final_prefix: *final_prefix,
            },
        })
    }
    fn send(self, session: &mut Session, now: i64) -> Result<OutboundFrame, MultiplayerError> {
        match self {
            Self::Scalar(message) => {
                session.send_progress(message.progress, message.final_prefix, now)
            }
            Self::Group {
                members,
                final_prefix,
            } => session.send_group_progress(members, final_prefix, now),
        }
    }
}

struct GroupOwnerState {
    local_roster: Vec<PlayerId>,
    local: Option<Vec<MemberProgress>>,
    remote_roster: Option<Vec<PlayerId>>,
    remote: Option<GroupPrefix>,
    remote_final: Option<GroupPrefix>,
}

fn copy_group_values<T: Copy>(values: &[T]) -> Result<Vec<T>, MultiplayerError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(values.len())
        .map_err(|_| MultiplayerError::Protocol("group owner allocation failed".into()))?;
    copy.extend_from_slice(values);
    Ok(copy)
}

fn copy_group_prefix(prefix: &GroupPrefix) -> Result<GroupPrefix, MultiplayerError> {
    Ok(GroupPrefix {
        sequence: prefix.sequence,
        final_prefix: prefix.final_prefix,
        members: copy_group_values(&prefix.members)?,
    })
}

/// Owned worker lifecycle. `stop` and Drop signal shutdown and join the worker.
/// Ordinary progress queue overflow terminates the session explicitly.
/// Terminal admission can retry capacity until the cleanup delivery deadline.
pub struct Multiplayer {
    outgoing: SyncSender<OutgoingMessage>,
    incoming: Receiver<MultiplayerNotice>,
    terminal: Receiver<MultiplayerError>,
    stop_flag: Arc<AtomicBool>,
    ready_requested: Arc<AtomicBool>,
    ready: bool,
    clock_epoch: Instant,
    clock_estimate: Option<OffsetEstimate>,
    start_schedule: Option<StartSchedule>,
    start_policy: StartPolicy,
    worker: Option<JoinHandle<()>>,
    local: Option<Progress>,
    remote: Option<Progress>,
    closed: bool,
    connected: bool,
    local_final: bool,
    remote_final: Option<Progress>,
    final_acknowledged: bool,
    finish_timeout: Duration,
    group: Option<GroupOwnerState>,
}
impl Multiplayer {
    /// Bind only the caller's explicit address. Binding errors are returned immediately.
    pub fn host(
        address: SocketAddr,
        identity: Vec<u8>,
        options: MultiplayerOptions,
    ) -> Result<Self, MultiplayerError> {
        Self::host_mode(address, identity, None, options)
    }
    pub fn join(
        address: SocketAddr,
        identity: Vec<u8>,
        options: MultiplayerOptions,
    ) -> Result<Self, MultiplayerError> {
        Self::join_mode(address, identity, None, options)
    }
    /// Connect to an HTTP/3 relay; the explicit role controls start negotiation,
    /// independently of both peers being network clients.
    pub fn webtransport(
        connection: WebTransportOptions,
        identity: Vec<u8>,
        options: MultiplayerOptions,
    ) -> Result<Self, MultiplayerError> {
        Self::webtransport_mode(connection, identity, None, options)
    }
    fn prepare_session(
        identity: Vec<u8>,
        players: Option<Vec<PlayerId>>,
        role: StartRole,
        options: &MultiplayerOptions,
    ) -> Result<(Session, Option<GroupOwnerState>), MultiplayerError> {
        validate_options(&identity, options)?;
        let session = match players {
            Some(players) => Session::new_group(
                identity,
                players,
                role,
                options.start_policy,
                options.preroll_ns,
            )?,
            None => Session::new(identity, role, options.start_policy, options.preroll_ns)?,
        };
        let group = match session.local_roster() {
            Some(players) => Some(GroupOwnerState {
                local_roster: copy_group_values(players)?,
                local: None,
                remote_roster: None,
                remote: None,
                remote_final: None,
            }),
            None => None,
        };
        Ok((session, group))
    }
    fn host_mode(
        address: SocketAddr,
        identity: Vec<u8>,
        players: Option<Vec<PlayerId>>,
        options: MultiplayerOptions,
    ) -> Result<Self, MultiplayerError> {
        let (session, group) = Self::prepare_session(identity, players, StartRole::Host, &options)?;
        let endpoint = Endpoint::Quic(QuicEndpoint::host(address, &options.quic)?);
        Self::spawn(endpoint, session, group, options)
    }
    fn join_mode(
        address: SocketAddr,
        identity: Vec<u8>,
        players: Option<Vec<PlayerId>>,
        options: MultiplayerOptions,
    ) -> Result<Self, MultiplayerError> {
        let (session, group) = Self::prepare_session(identity, players, StartRole::Join, &options)?;
        let endpoint = Endpoint::Quic(QuicEndpoint::join(address, &options.quic)?);
        Self::spawn(endpoint, session, group, options)
    }
    fn webtransport_mode(
        connection: WebTransportOptions,
        identity: Vec<u8>,
        players: Option<Vec<PlayerId>>,
        options: MultiplayerOptions,
    ) -> Result<Self, MultiplayerError> {
        #[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
        {
            let (session, group) =
                Self::prepare_session(identity, players, connection.role, &options)?;
            if options.quic.cert.is_some()
                || options.quic.key.is_some()
                || options.quic.server_name.is_some()
                || options
                    .quic
                    .ca
                    .as_ref()
                    .is_some_and(|ca| ca != &connection.ca)
            {
                return Err(MultiplayerError::InvalidOptions);
            }
            let endpoint = Endpoint::WebTransport(WebTransportEndpoint::prepare(&connection)?);
            Self::spawn(endpoint, session, group, options)
        }
        #[cfg(not(all(not(target_arch = "wasm32"), feature = "webtransport")))]
        {
            let _ = (connection, identity, players, options);
            Err(crate::multiplayer_webtransport_client::unavailable().into())
        }
    }
    fn spawn(
        endpoint: Endpoint,
        session: Session,
        group: Option<GroupOwnerState>,
        options: MultiplayerOptions,
    ) -> Result<Self, MultiplayerError> {
        let (outgoing, out_rx) = mpsc::sync_channel(options.queue_capacity);
        let (in_tx, incoming) = mpsc::sync_channel(options.queue_capacity);
        // Separate terminal slot remains available even when the data queue is full.
        let (terminal_tx, terminal) = mpsc::sync_channel(1);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let worker_stop = stop_flag.clone();
        let ready_requested = Arc::new(AtomicBool::new(false));
        let worker_ready = ready_requested.clone();
        let finish_timeout = options.io_stall_timeout;
        let start_policy = options.start_policy;
        let clock_epoch = Instant::now();
        let worker = thread::Builder::new()
            .name("bms-multiplayer".into())
            .spawn(move || {
                let result = run(
                    endpoint,
                    session,
                    options,
                    &worker_stop,
                    &worker_ready,
                    clock_epoch,
                    out_rx,
                    in_tx,
                );
                let reason = result.err().unwrap_or(MultiplayerError::Closed);
                let _ = terminal_tx.try_send(reason);
            })?;
        Ok(Self {
            outgoing,
            incoming,
            terminal,
            stop_flag,
            ready_requested,
            ready: false,
            clock_epoch,
            clock_estimate: None,
            start_schedule: None,
            start_policy,
            worker: Some(worker),
            local: None,
            remote: None,
            closed: false,
            connected: false,
            local_final: false,
            remote_final: None,
            final_acknowledged: false,
            finish_timeout,
            group,
        })
    }
    /// One-shot nonblocking preparation-ready request, independent of data capacity.
    /// May precede setup exchange; only the worker sends after compatible setup.
    pub fn try_ready(&mut self) -> Result<(), MultiplayerError> {
        if self.closed || self.stop_flag.load(Ordering::Acquire) {
            return Err(MultiplayerError::Closed);
        }
        self.ready_requested
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| MultiplayerError::Protocol("readiness already requested".into()))?;
        if let Some(worker) = &self.worker {
            worker.thread().unpark();
        }
        Ok(())
    }
    /// True after bilateral readiness is polled and until closure.
    pub fn is_ready(&self) -> bool {
        self.ready && !self.closed
    }
    /// Session monotonic epoch shared with this owner's socket worker.
    pub fn clock_now_ns(&self) -> Result<i64, MultiplayerError> {
        elapsed_ns(self.clock_epoch)
    }
    pub fn clock_estimate(&self) -> Option<OffsetEstimate> {
        self.clock_estimate
    }
    pub fn start_schedule(&self) -> Option<StartSchedule> {
        self.start_schedule
    }
    pub fn start_policy(&self) -> StartPolicy {
        self.start_policy
    }
    /// Gameplay-side admission only; performs no socket I/O and never waits for capacity.
    pub fn try_publish(&mut self, progress: Progress) -> Result<(), MultiplayerError> {
        self.admit(OutgoingMessage::Scalar(Outgoing {
            progress,
            final_prefix: false,
        }))
    }
    /// Nonblocking terminal admission, immutable after success. Full is retryable.
    pub fn try_finish(&mut self, progress: Progress) -> Result<(), MultiplayerError> {
        self.admit(OutgoingMessage::Scalar(Outgoing {
            progress,
            final_prefix: true,
        }))
    }
    fn validate_publication(&self, message: &OutgoingMessage) -> Result<(), MultiplayerError> {
        if self.local_final {
            return Err(MultiplayerError::Protocol(
                "local final already admitted".into(),
            ));
        }
        match (self.group.as_ref(), message) {
            (None, OutgoingMessage::Scalar(message)) => {
                validate_progress(self.local, message.progress)
            }
            (Some(group), OutgoingMessage::Group { members, .. }) => {
                validate_members(group.local.as_deref(), members)?;
                if group.local_roster.len() != members.len()
                    || group
                        .local_roster
                        .iter()
                        .zip(members)
                        .any(|(player, member)| *player != member.player)
                {
                    return Err(MultiplayerError::Protocol(
                        "group progress changed the local roster".into(),
                    ));
                }
                Ok(())
            }
            _ => Err(MultiplayerError::Protocol(
                "publication mode differs from the session".into(),
            )),
        }
    }
    fn admit(&mut self, message: OutgoingMessage) -> Result<(), MultiplayerError> {
        if self.closed || self.stop_flag.load(Ordering::Acquire) {
            return Err(MultiplayerError::Closed);
        }
        if !self.is_ready() || self.start_schedule.is_none() {
            return Err(MultiplayerError::Protocol(
                "committed start required".into(),
            ));
        }
        self.validate_publication(&message)?;
        // Allocate the retained whole prefix before queue admission. Neither
        // capacity refusal nor a later invalid member can change the old prefix.
        let retained = message.try_clone()?;
        let final_prefix = message.final_prefix();
        match self.outgoing.try_send(message) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                if !final_prefix {
                    self.signal_stop();
                    self.closed = true;
                }
                return Err(MultiplayerError::QueueFull);
            }
            Err(TrySendError::Disconnected(_)) => {
                if !final_prefix {
                    self.closed = true;
                }
                return Err(MultiplayerError::Closed);
            }
        }
        match retained {
            OutgoingMessage::Scalar(message) => self.local = Some(message.progress),
            OutgoingMessage::Group { members, .. } => {
                if let Some(group) = &mut self.group {
                    group.local = Some(members);
                }
            }
        }
        self.local_final = final_prefix;
        if let Some(worker) = &self.worker {
            worker.thread().unpark();
        }
        Ok(())
    }
    /// Cleanup-only bounded final delivery; performs no socket I/O on the caller.
    /// Call after native resources are stopped. Immediate cancellation still wins.
    pub fn finish_delivery(&mut self, progress: Progress) -> Result<(), MultiplayerError> {
        self.validate_publication(&OutgoingMessage::Scalar(Outgoing {
            progress,
            final_prefix: true,
        }))?;
        let deadline = Instant::now()
            .checked_add(self.finish_timeout)
            .ok_or_else(|| {
                MultiplayerError::Protocol("final wait deadline extent exceeded".into())
            })?;
        self.wait_for_delivery(progress, deadline)
    }
    fn wait_for_delivery(
        &mut self,
        progress: Progress,
        deadline: Instant,
    ) -> Result<(), MultiplayerError> {
        self.wait_for_notice_delivery(
            OutgoingMessage::Scalar(Outgoing {
                progress,
                final_prefix: true,
            }),
            deadline,
        )
    }
    fn wait_for_notice_delivery(
        &mut self,
        message: OutgoingMessage,
        deadline: Instant,
    ) -> Result<(), MultiplayerError> {
        let already_admitted = self.local_final;
        let (mut control, deadline_ns) =
            crate::native_final_wait_bridge::NativeFinalWaitControl::until(deadline)?;
        crate::final_ack_wait::wait_for_final_ack(
            &mut NativeFinalAckPort {
                owner: self,
                message: &message,
            },
            &mut control,
            deadline_ns,
            already_admitted,
        )
        .map_err(|error| match error {
            crate::final_ack_wait::FinalAckWaitError::Port(error)
            | crate::final_ack_wait::FinalAckWaitError::Control(error) => error,
            crate::final_ack_wait::FinalAckWaitError::Cancelled
            | crate::final_ack_wait::FinalAckWaitError::Closed => MultiplayerError::Closed,
            crate::final_ack_wait::FinalAckWaitError::TimedOut => MultiplayerError::IoStalled,
            crate::final_ack_wait::FinalAckWaitError::ClockRegressed => {
                MultiplayerError::Protocol("final wait clock regressed".into())
            }
        })
    }
    fn retain_event(&mut self, event: &MultiplayerEvent) {
        match event {
            MultiplayerEvent::Connected => self.connected = true,
            MultiplayerEvent::Ready => self.ready = true,
            MultiplayerEvent::ClockEstimated(estimate) => self.clock_estimate = Some(*estimate),
            MultiplayerEvent::StartScheduled(schedule) => self.start_schedule = Some(*schedule),
            MultiplayerEvent::Progress(progress) => self.remote = Some(*progress),
            MultiplayerEvent::FinalProgress(progress) => {
                self.remote = Some(*progress);
                self.remote_final = Some(*progress);
            }
            MultiplayerEvent::FinalAcknowledged => self.final_acknowledged = true,
            MultiplayerEvent::Disconnected(_) => {}
        }
    }
    fn retain_notice(&mut self, notice: &MultiplayerNotice) -> Result<(), MultiplayerError> {
        match notice {
            MultiplayerNotice::Session(event) => self.retain_event(event),
            MultiplayerNotice::Group(event) => {
                let group = self.group.as_mut().ok_or_else(|| {
                    MultiplayerError::Protocol("group observation on a scalar owner".into())
                })?;
                match event {
                    GroupEvent::Roster(players) => {
                        group.remote_roster = Some(copy_group_values(players)?)
                    }
                    GroupEvent::Progress(prefix) => {
                        let remote = copy_group_prefix(prefix)?;
                        let final_prefix = if prefix.final_prefix {
                            Some(copy_group_prefix(prefix)?)
                        } else {
                            None
                        };
                        group.remote = Some(remote);
                        if let Some(prefix) = final_prefix {
                            group.remote_final = Some(prefix);
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn drain_notices(
        &mut self,
        events: &mut Vec<MultiplayerNotice>,
    ) -> Result<(), MultiplayerError> {
        // Worker may keep producing, so retain an explicit per-call bound.
        for _ in 0..1024 {
            match self.incoming.try_recv() {
                Ok(event) => {
                    let retained = self.retain_notice(&event);
                    events.push(event);
                    if let Err(error) = retained {
                        self.signal_stop();
                        return Err(error);
                    }
                }
                Err(_) => break,
            }
        }
        Ok(())
    }
    /// Drain a bounded batch; terminal notification follows all retained remote data.
    pub fn poll(&mut self) -> Vec<MultiplayerEvent> {
        self.poll_notices()
            .into_iter()
            .filter_map(|notice| match notice {
                MultiplayerNotice::Session(event) => Some(event),
                MultiplayerNotice::Group(_) => None,
            })
            .collect()
    }
    fn poll_notices(&mut self) -> Vec<MultiplayerNotice> {
        let mut events = Vec::new();
        let failure = self.drain_notices(&mut events).err();
        let terminal = failure.or_else(|| match self.terminal.try_recv() {
            Ok(reason) => Some(reason),
            Err(TryRecvError::Disconnected) if !self.closed => {
                Some(MultiplayerError::WorkerPanicked)
            }
            Err(_) => None,
        });
        if let Some(reason) = terminal {
            // The worker has exited before publishing its terminal slot. Drain its
            // remaining bounded queue so no progress is emitted after disconnect.
            let reason = self.drain_notices(&mut events).err().unwrap_or(reason);
            self.closed = true;
            self.connected = false;
            events.push(MultiplayerNotice::Session(MultiplayerEvent::Disconnected(
                reason,
            )));
        }
        events
    }
    /// True after the Connected event has been polled and until closure.
    pub fn is_connected(&self) -> bool {
        self.connected && !self.closed
    }
    pub fn remote_progress(&self) -> Option<Progress> {
        self.remote
    }
    /// Last received peer terminal prefix, retained across stop/disconnect.
    pub fn remote_final_progress(&self) -> Option<Progress> {
        self.remote_final
    }
    fn signal_stop(&self) {
        self.stop_flag.store(true, Ordering::Release);
        if let Some(worker) = &self.worker {
            worker.thread().unpark();
        }
    }
    /// Signal cancellation without joining; keep this owner until native cleanup.
    pub fn request_stop(&mut self) {
        self.signal_stop();
        self.closed = true;
        self.connected = false;
    }
    /// Joins after bounded setup cancellation or connected send-drain cleanup,
    /// plus worker scheduling. This is never an input/audio callback operation.
    pub fn stop(&mut self) -> Result<(), MultiplayerError> {
        self.signal_stop();
        self.closed = true;
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| MultiplayerError::WorkerPanicked)?;
        }
        Ok(())
    }
}
struct NativeFinalAckPort<'a> {
    owner: &'a mut Multiplayer,
    message: &'a OutgoingMessage,
}
impl crate::final_ack_wait::FinalAckPort for NativeFinalAckPort<'_> {
    type Error = MultiplayerError;
    fn poll(
        &mut self,
    ) -> Result<crate::final_ack_wait::FinalAckObservation<Self::Error>, Self::Error> {
        use crate::final_ack_wait::{FinalAckObservation, FinalAckFailure};
        let mut failure = None;
        for event in self.owner.poll_notices() {
            if let MultiplayerNotice::Session(MultiplayerEvent::Disconnected(error)) = event {
                failure = Some(if matches!(error, MultiplayerError::Closed) {
                    FinalAckFailure::Closed(error)
                } else {
                    FinalAckFailure::Other(error)
                });
            }
        }
        Ok(FinalAckObservation {
            cancelled: self.owner.stop_flag.load(Ordering::Acquire),
            acknowledged: self.owner.final_acknowledged,
            closed: self.owner.closed,
            failure,
        })
    }
    fn admit(&mut self) -> Result<crate::final_ack_wait::FinalAdmission, Self::Error> {
        use crate::final_ack_wait::FinalAdmission;
        match self.owner.admit(self.message.try_clone()?) {
            Ok(()) => Ok(FinalAdmission::Accepted),
            Err(MultiplayerError::QueueFull) => Ok(FinalAdmission::QueueFull),
            Err(error) => Err(error),
        }
    }
}

impl Drop for Multiplayer {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Whole-cohort facade over the same worker and application acknowledgement lifecycle.
/// Rows are unauthenticated peer reports, never an aggregate or a scalar alias.
pub struct GroupMultiplayer {
    owner: Multiplayer,
}
impl GroupMultiplayer {
    pub fn host(
        address: SocketAddr,
        identity: Vec<u8>,
        players: Vec<PlayerId>,
        options: MultiplayerOptions,
    ) -> Result<Self, MultiplayerError> {
        Ok(Self {
            owner: Multiplayer::host_mode(address, identity, Some(players), options)?,
        })
    }
    pub fn join(
        address: SocketAddr,
        identity: Vec<u8>,
        players: Vec<PlayerId>,
        options: MultiplayerOptions,
    ) -> Result<Self, MultiplayerError> {
        Ok(Self {
            owner: Multiplayer::join_mode(address, identity, Some(players), options)?,
        })
    }
    pub fn webtransport(
        connection: WebTransportOptions,
        identity: Vec<u8>,
        players: Vec<PlayerId>,
        options: MultiplayerOptions,
    ) -> Result<Self, MultiplayerError> {
        Ok(Self {
            owner: Multiplayer::webtransport_mode(connection, identity, Some(players), options)?,
        })
    }
    pub fn try_ready(&mut self) -> Result<(), MultiplayerError> {
        self.owner.try_ready()
    }
    pub fn is_ready(&self) -> bool {
        self.owner.is_ready()
    }
    pub fn clock_now_ns(&self) -> Result<i64, MultiplayerError> {
        self.owner.clock_now_ns()
    }
    pub fn clock_estimate(&self) -> Option<OffsetEstimate> {
        self.owner.clock_estimate()
    }
    pub fn start_schedule(&self) -> Option<StartSchedule> {
        self.owner.start_schedule()
    }
    pub fn start_policy(&self) -> StartPolicy {
        self.owner.start_policy()
    }
    pub fn is_connected(&self) -> bool {
        self.owner.is_connected()
    }
    pub fn request_stop(&mut self) {
        self.owner.request_stop();
    }
    pub fn stop(&mut self) -> Result<(), MultiplayerError> {
        self.owner.stop()
    }
    pub fn local_roster(&self) -> &[PlayerId] {
        self.owner
            .group
            .as_ref()
            .map(|group| group.local_roster.as_slice())
            .unwrap_or(&[])
    }
    pub fn remote_roster(&self) -> Option<&[PlayerId]> {
        self.owner
            .group
            .as_ref()
            .and_then(|group| group.remote_roster.as_deref())
    }
    pub fn remote_progress(&self) -> Option<&GroupPrefix> {
        self.owner
            .group
            .as_ref()
            .and_then(|group| group.remote.as_ref())
    }
    pub fn remote_final_progress(&self) -> Option<&GroupPrefix> {
        self.owner
            .group
            .as_ref()
            .and_then(|group| group.remote_final.as_ref())
    }
    pub fn try_publish(&mut self, members: Vec<MemberProgress>) -> Result<(), MultiplayerError> {
        self.owner.admit(OutgoingMessage::Group {
            members,
            final_prefix: false,
        })
    }
    pub fn try_finish(&mut self, members: Vec<MemberProgress>) -> Result<(), MultiplayerError> {
        self.owner.admit(OutgoingMessage::Group {
            members,
            final_prefix: true,
        })
    }
    /// Cleanup-only delivery of this immutable whole prefix, confirmed by the
    /// real final acknowledgement. Queue capacity refusal alone is retryable.
    pub fn finish_delivery(
        &mut self,
        members: Vec<MemberProgress>,
    ) -> Result<(), MultiplayerError> {
        let message = OutgoingMessage::Group {
            members,
            final_prefix: true,
        };
        self.owner.validate_publication(&message)?;
        let deadline = Instant::now()
            .checked_add(self.owner.finish_timeout)
            .ok_or_else(|| {
                MultiplayerError::Protocol("final wait deadline extent exceeded".into())
            })?;
        self.owner.wait_for_notice_delivery(message, deadline)
    }
    pub fn poll(&mut self) -> Vec<MultiplayerNotice> {
        self.owner.poll_notices()
    }
}

fn elapsed_ns(epoch: Instant) -> Result<i64, MultiplayerError> {
    i64::try_from(epoch.elapsed().as_nanos())
        .map_err(|_| MultiplayerError::Protocol("session clock extent exceeded".into()))
}

fn forward_session_events(
    session: &mut Session,
    incoming: &SyncSender<MultiplayerNotice>,
) -> Result<(), MultiplayerError> {
    while let Some(event) = session
        .poll_group_event()
        .map(MultiplayerNotice::Group)
        .or_else(|| session.poll_event().map(MultiplayerNotice::Session))
    {
        incoming.try_send(event).map_err(queue_error)?;
    }
    Ok(())
}

fn run(
    endpoint: Endpoint,
    mut session: Session,
    options: MultiplayerOptions,
    stop: &AtomicBool,
    ready_requested: &AtomicBool,
    clock_epoch: Instant,
    outgoing: Receiver<OutgoingMessage>,
    incoming: SyncSender<MultiplayerNotice>,
) -> Result<(), MultiplayerError> {
    let deadline = Instant::now() + options.setup_timeout;
    let mut stream = match endpoint.connect(stop, deadline) {
        Ok(stream) => stream,
        Err(_) if stop.load(Ordering::Acquire) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::TimedOut => {
            return Err(MultiplayerError::SetupTimeout);
        }
        Err(error) => return Err(error.into()),
    };
    let result = (|| {
        let mut tx = match session.poll_write(elapsed_ns(clock_epoch)?)? {
            WriteStep::Frame(frame) => Some(frame),
            _ => {
                return Err(MultiplayerError::Protocol(
                    "missing initial setup frame".into(),
                ));
            }
        };
        let mut written = 0;
        let mut rx = Frames::new();
        let mut buffer = [0; 4096];
        let mut last_write = Instant::now();
        let mut last_read = Instant::now();
        let mut readiness_requested = false;
        loop {
            if stop.load(Ordering::Acquire) {
                return Ok(());
            }
            if session.preparation_pending() && Instant::now() >= deadline {
                return Err(MultiplayerError::SetupTimeout);
            }
            if session.setup_complete()
                && ((tx.is_some() && last_write.elapsed() >= options.io_stall_timeout)
                    || (!rx.bytes.is_empty() && last_read.elapsed() >= options.io_stall_timeout))
            {
                return Err(MultiplayerError::IoStalled);
            }
            if !readiness_requested && ready_requested.load(Ordering::Acquire) {
                session.request_ready()?;
                readiness_requested = true;
            }
            if let Some(frame) = &tx {
                match stream.write(&frame.bytes[written..]) {
                    Ok(0) => return Err(MultiplayerError::Closed),
                    Ok(count) => {
                        written += count;
                        last_write = Instant::now();
                        if written == frame.bytes.len() {
                            session.written(frame.id, elapsed_ns(clock_epoch)?)?;
                            tx = None;
                            written = 0;
                            // Publish complete-write effects before a following
                            // read can see peer EOF, including the final ACK.
                            forward_session_events(&mut session, &incoming)?;
                        }
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) => return Err(error.into()),
                }
            }
            let needed = rx.needed()?.min(buffer.len());
            if needed != 0 {
                match stream.read(&mut buffer[..needed]) {
                    Ok(0) => return Err(MultiplayerError::Closed),
                    Ok(count) => {
                        let consumed = rx.push(&buffer[..count])?;
                        if consumed != count {
                            return Err(MultiplayerError::Protocol(
                                "decoder did not admit bounded transport read".into(),
                            ));
                        }
                        last_read = Instant::now();
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) => return Err(error.into()),
                }
            }
            if let Some((tag, payload)) = rx.take()? {
                session.receive(tag, &payload, elapsed_ns(clock_epoch)?)?;
                forward_session_events(&mut session, &incoming)?;
            }
            if tx.is_none() {
                let next = match session.poll_write(elapsed_ns(clock_epoch)?)? {
                    WriteStep::Frame(frame) => Some(frame),
                    WriteStep::ApplicationSlot => match outgoing.try_recv() {
                        Ok(message) => Some(message.send(&mut session, elapsed_ns(clock_epoch)?)?),
                        Err(TryRecvError::Empty) => None,
                        Err(TryRecvError::Disconnected) => return Ok(()),
                    },
                    WriteStep::Waiting => None,
                };
                if let Some(frame) = next {
                    tx = Some(frame);
                    written = 0;
                    last_write = Instant::now();
                }
            }
            forward_session_events(&mut session, &incoming)?;
            stream.idle(TICK);
        }
    })();
    // QUIC close may discard buffered stream bytes. Keep the endpoint/driver
    // alive for bounded transport receipt; this is not a peer application ACK.
    let drained = stream
        .finish(options.io_stall_timeout)
        .map_err(MultiplayerError::from);
    result.and(drained)
}

fn queue_error<T>(error: TrySendError<T>) -> MultiplayerError {
    match error {
        TrySendError::Full(_) => MultiplayerError::QueueFull,
        TrySendError::Disconnected(_) => MultiplayerError::Closed,
    }
}

#[cfg(test)]
#[path = "multiplayer_native_group_fixtures.rs"]
mod native_group_fixtures;

#[cfg(test)]
mod clock_probe_fixtures {
    use super::*;

    fn decode(wire: &[u8], expected_tag: u8) -> Vec<u8> {
        let mut decoder = Frames::new();
        for (index, byte) in wire.iter().enumerate() {
            decoder.bytes.push(*byte);
            let parsed = decoder.take().unwrap();
            if index + 1 == wire.len() {
                let (tag, payload) = parsed.unwrap();
                assert_eq!(tag, expected_tag);
                return payload;
            }
            assert!(parsed.is_none());
        }
        panic!("missing frame")
    }

    #[test]
    fn eight_fragmented_exchanges_select_minimum_delay_and_emit_once() {
        let mut local = ClockProbes::default();
        let mut peer = ClockProbes::default();
        for sequence in 0..8 {
            let sent = 1_000 + sequence * 1_000;
            let (forward, reverse) = if sequence == 3 { (3, 7) } else { (20, 30) };
            let received = sent + forward + 100;
            let replied = received + 5;
            let arrived = sent + forward + 5 + reverse;
            let ping = local.next_ping(sent).unwrap().unwrap();
            assert!(local.next_ping(sent).unwrap().is_none());
            peer.receive_ping(&decode(&ping, 6), received).unwrap();
            let pong = peer.next_pong(replied).unwrap().unwrap();
            assert!(peer.next_pong(replied).unwrap().is_none());
            local.receive_pong(&decode(&pong, 7), arrived).unwrap();
            if sequence < 7 {
                assert!(local.estimate_event().is_none());
            }
        }
        let Some(MultiplayerEvent::ClockEstimated(estimate)) = local.estimate_event() else {
            panic!("missing estimate")
        };
        assert_eq!(
            (
                estimate.lower_ns(),
                estimate.upper_ns(),
                estimate.round_trip_ns()
            ),
            (93, 103, 10)
        );
        assert_eq!(estimate.observed_local_ns(), 4_015);
        let deadline = estimate
            .remote_deadline_to_local(10_100, 8_100, 5_000)
            .unwrap();
        assert_eq!(
            (deadline.earliest_ns(), deadline.latest_ns()),
            (9_997, 10_007)
        );
        assert!(
            estimate
                .remote_deadline_to_local(10_100, 9_016, 5_000)
                .is_err()
        );
        assert!(local.estimate_event().is_none());
        assert!(local.next_ping(9_000).unwrap().is_none());
        assert!(
            peer.receive_ping(&decode(&frame(6, &[0; 16]), 6), 9_000)
                .is_err()
        );
    }

    #[test]
    fn malformed_unsolicited_and_mismatched_pongs_do_not_consume_pending_probe() {
        let mut local = ClockProbes::default();
        let mut peer = ClockProbes::default();
        assert!(local.receive_pong(&[0; 32], 100).is_err());
        let ping = decode(&local.next_ping(100).unwrap().unwrap(), 6);
        assert!(peer.receive_ping(&ping[..15], 160).is_err());
        peer.receive_ping(&ping, 160).unwrap();
        assert!(peer.receive_ping(&ping, 160).is_err());
        assert!(peer.next_pong(159).is_err());
        let pong = decode(&peer.next_pong(180).unwrap().unwrap(), 7);
        for offset in [0, 8] {
            let mut wrong = pong.clone();
            wrong[offset] ^= 1;
            assert!(local.receive_pong(&wrong, 140).is_err());
            assert_eq!(local.pending_ping, Some((0, 100)));
            assert_eq!(local.completed, 0);
            assert!(local.filter.estimate().is_none());
        }
        assert!(local.receive_pong(&pong[..31], 140).is_err());
        assert!(local.receive_pong(&pong, 110).is_err()); // processing exceeds local span
        local.receive_pong(&pong, 140).unwrap();
        assert_eq!(local.completed, 1);
        assert!(local.receive_pong(&pong, 140).is_err());
        assert!(local.next_ping(139).is_err());
        assert!(local.next_ping(140).unwrap().is_some());
    }

    #[test]
    fn simultaneous_outstanding_probes_keep_independent_epoch_signs() {
        let mut left = ClockProbes::default();
        let mut right = ClockProbes::default();
        let left_ping = decode(&left.next_ping(100).unwrap().unwrap(), 6);
        let right_ping = decode(&right.next_ping(150).unwrap().unwrap(), 6);
        right.receive_ping(&left_ping, 160).unwrap();
        left.receive_ping(&right_ping, 110).unwrap();
        let left_pong = decode(&left.next_pong(120).unwrap().unwrap(), 7);
        let right_pong = decode(&right.next_pong(170).unwrap().unwrap(), 7);
        left.receive_pong(&right_pong, 130).unwrap();
        right.receive_pong(&left_pong, 180).unwrap();
        let positive = left.filter.estimate().unwrap();
        let negative = right.filter.estimate().unwrap();
        assert_eq!((positive.lower_ns(), positive.upper_ns()), (40, 60));
        assert_eq!((negative.lower_ns(), negative.upper_ns()), (-60, -40));
        assert_eq!(left.pending_ping, None);
        assert_eq!(right.pending_ping, None);
    }

    #[test]
    fn start_wire_fragmentation_exact_lengths_and_echo_preservation() {
        for message in [
            StartMessage::ClockReady(123),
            StartMessage::Propose(i64::MAX),
            StartMessage::Accept(123),
            StartMessage::Commit(123),
        ] {
            let wire = start_frame(message);
            let payload = decode(&wire, wire[10]);
            assert_eq!(parse_start_frame(wire[10], &payload).unwrap(), message);
        }
        assert!(parse_start_frame(8, &[0]).is_err());
        assert!(parse_start_frame(8, &[]).is_err());
        for tag in 9..=11 {
            assert!(parse_start_frame(tag, &[]).is_err());
            assert!(parse_start_frame(tag, &[0; 7]).is_err());
            assert!(parse_start_frame(tag, &[0; 9]).is_err());
        }
        assert!(parse_start_frame(12, &[0; 8]).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn section_header(start: i64, seed: u64) -> ReplayHeader {
        let mut options = if seed != 0 {
            b"bms-judge-profile/v3:".to_vec()
        } else if start != 0 {
            b"bms-judge-profile/v2:".to_vec()
        } else {
            b"bms-judge-profile/v1:".to_vec()
        };
        if seed != 0 {
            options.extend_from_slice(&seed.to_le_bytes());
        }
        if seed != 0 || start != 0 {
            options.extend_from_slice(&start.to_le_bytes());
        }
        options.extend_from_slice(&(-9_i64).to_le_bytes());
        options.extend_from_slice(&1_u64.to_le_bytes());
        options.extend_from_slice(&1_u32.to_le_bytes());
        options.extend_from_slice(&10_i64.to_le_bytes());
        options.extend_from_slice(&20_i64.to_le_bytes());
        ReplayHeader {
            version: beatkernel::replay::REPLAY_VERSION,
            chart_identity: vec![1],
            rules_identity: vec![2],
            options,
            seed: 0,
            normalized_clock: ClockDomainId(17),
        }
    }
    fn section_limits() -> ReplayCodecLimits {
        ReplayCodecLimits::new(
            131072,
            1,
            131000,
            beatkernel::input::CodecLimits::new(64, 0).unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn literal_section_envelope_preserves_none_and_only_normalizes_clock() {
        let mut header = section_header(13, u64::MAX);
        let limits = section_limits();
        let legacy = competition_identity(&header, "runtime", limits).unwrap();
        assert_eq!(
            competition_identity_for_section(&header, "runtime", limits, None).unwrap(),
            legacy
        );
        let end = Timestamp::from_nanos(i64::MAX);
        let mut expected = b"bms-competition-section/v1:".to_vec();
        expected.extend_from_slice(&i64::MAX.to_le_bytes());
        expected.extend_from_slice(&legacy);
        let actual =
            competition_identity_for_section(&header, "runtime", limits, Some(end)).unwrap();
        assert_eq!(actual, expected);
        assert_ne!(actual, legacy);
        header.normalized_clock = ClockDomainId(99);
        assert_eq!(
            competition_identity_for_section(&header, "runtime", limits, Some(end)).unwrap(),
            actual
        );
        assert_ne!(
            competition_identity_for_section(
                &header,
                "runtime",
                limits,
                Some(Timestamp::from_nanos(i64::MAX - 1))
            )
            .unwrap(),
            actual
        );
        assert_ne!(
            competition_identity_for_section(&header, "other-runtime", limits, Some(end)).unwrap(),
            actual
        );
        for mut changed in [
            section_header(14, u64::MAX),
            section_header(13, 3),
            section_header(13, 0),
        ] {
            assert_ne!(
                competition_identity_for_section(&changed, "runtime", limits, Some(end)).unwrap(),
                actual
            );
            changed.rules_identity.push(3);
            assert_ne!(
                competition_identity_for_section(&changed, "runtime", limits, Some(end)).unwrap(),
                actual
            );
        }
        let mut changed = header.clone();
        changed.rules_identity.push(3);
        assert_ne!(
            competition_identity_for_section(&changed, "runtime", limits, Some(end)).unwrap(),
            actual
        );
        let prefix = b"bms-judge-profile/v3:".len();
        let mut changed = header.clone();
        changed.options[prefix + 16..prefix + 24].copy_from_slice(&(-8_i64).to_le_bytes());
        assert_ne!(
            competition_identity_for_section(&changed, "runtime", limits, Some(end)).unwrap(),
            actual
        );
        let mut changed = header;
        changed.seed = 1;
        assert_ne!(
            competition_identity_for_section(&changed, "runtime", limits, Some(end)).unwrap(),
            actual
        );
    }
    #[test]
    fn finite_metadata_and_endpoint_errors_are_explicit() {
        let header = section_header(13, 3);
        let limits = section_limits();
        for end in [-1, 0, 12, 13] {
            assert!(
                competition_identity_for_section(
                    &header,
                    "runtime",
                    limits,
                    Some(Timestamp::from_nanos(end))
                )
                .is_err()
            );
        }
        let mut malformed = vec![section_header(-1, 3), section_header(-1, 0)];
        let mut zero_seed = header.clone();
        let prefix = b"bms-judge-profile/v3:".len();
        zero_seed.options[prefix..prefix + 8].fill(0);
        malformed.push(zero_seed);
        let mut short = header.clone();
        short.options.truncate(prefix + 15);
        malformed.push(short);
        let mut trailing = header.clone();
        trailing.options.push(0);
        malformed.push(trailing);
        let mut count = header.clone();
        count.options[prefix + 24..prefix + 32].copy_from_slice(&u64::MAX.to_le_bytes());
        malformed.push(count);
        let mut unknown = header.clone();
        unknown.options = b"unknown".to_vec();
        malformed.push(unknown);
        for bad in malformed {
            assert!(
                competition_identity_for_section(
                    &bad,
                    "runtime",
                    limits,
                    Some(Timestamp::from_nanos(14))
                )
                .is_err()
            );
        }
        let tight = ReplayCodecLimits::new(
            1024,
            1,
            1,
            beatkernel::input::CodecLimits::new(64, 0).unwrap(),
        )
        .unwrap();
        assert!(
            competition_identity_for_section(
                &header,
                "runtime",
                tight,
                Some(Timestamp::from_nanos(14))
            )
            .is_err()
        );
    }
    #[test]
    fn whole_section_identity_obeys_exact_max_after_wrapping() {
        let mut header = section_header(0, 0);
        let limits = section_limits();
        let overhead = b"bms-competition-section/v1:".len() + 8;
        let base = competition_identity(&header, "runtime", limits)
            .unwrap()
            .len();
        header
            .rules_identity
            .resize(1 + MAX_IDENTITY - overhead - base, 2);
        let end = Some(Timestamp::from_nanos(1));
        assert_eq!(
            competition_identity_for_section(&header, "runtime", limits, end)
                .unwrap()
                .len(),
            MAX_IDENTITY
        );
        header.rules_identity.push(2);
        assert!(competition_identity(&header, "runtime", limits).is_ok());
        assert!(competition_identity_for_section(&header, "runtime", limits, end).is_err());
    }
    fn progress() -> Progress {
        Progress {
            song_ns: -2,
            hits: 2,
            misses: 1,
            combo: 1,
            max_combo: 2,
        }
    }
    #[test]
    fn fragment_roundtrip_and_ordering() {
        let value = progress();
        let wire = progress_frame(0, value);
        let mut decoder = Frames::new();
        for byte in &wire[..wire.len() - 1] {
            decoder.bytes.push(*byte);
            assert!(decoder.take().unwrap().is_none());
        }
        decoder.bytes.push(*wire.last().unwrap());
        let (tag, payload) = decoder.take().unwrap().unwrap();
        assert_eq!(tag, 2);
        assert_eq!(parse_progress(&payload, 0, None).unwrap(), value);
        assert!(parse_progress(&payload, 1, None).is_err());
        let equal = Progress {
            hits: 3,
            combo: 2,
            ..value
        };
        assert!(validate_progress(Some(value), equal).is_ok());
        assert!(
            validate_progress(
                Some(value),
                Progress {
                    song_ns: -3,
                    ..value
                }
            )
            .is_err()
        );
        assert!(validate_progress(Some(value), Progress { combo: 2, ..value }).is_err());
    }
    #[test]
    fn finite_queue_and_timeout_configuration() {
        let (sender, receiver) = mpsc::sync_channel(1);
        sender.try_send(progress()).unwrap();
        assert_eq!(
            queue_error(sender.try_send(progress()).unwrap_err()),
            MultiplayerError::QueueFull
        );
        drop(receiver);
        assert_eq!(
            queue_error(sender.try_send(progress()).unwrap_err()),
            MultiplayerError::Closed
        );
        let mut options = MultiplayerOptions::default();
        assert!(validate_options(&[1], &options).is_ok());
        options.io_stall_timeout = Duration::ZERO;
        assert_eq!(
            validate_options(&[1], &options),
            Err(MultiplayerError::InvalidOptions)
        );
    }
    #[test]
    fn exact_identity_ignores_only_capture_clock() {
        let limits = ReplayCodecLimits::new(
            65536,
            1,
            4096,
            beatkernel::input::CodecLimits::new(64, 0).unwrap(),
        )
        .unwrap();
        let mut header = ReplayHeader {
            version: beatkernel::replay::REPLAY_VERSION,
            chart_identity: vec![1],
            rules_identity: vec![2],
            options: vec![3],
            seed: 4,
            normalized_clock: ClockDomainId(5),
        };
        let identity = competition_identity(&header, "test-runtime", limits).unwrap();
        header.normalized_clock = ClockDomainId(91);
        assert_eq!(
            identity,
            competition_identity(&header, "test-runtime", limits).unwrap()
        );
        assert_ne!(
            identity,
            competition_identity(&header, "other-runtime", limits).unwrap()
        );
        header.seed += 1;
        assert_ne!(
            identity,
            competition_identity(&header, "test-runtime", limits).unwrap()
        );
    }
    #[test]
    fn hostile_lengths_versions_and_counts() {
        let mut decoder = Frames::new();
        decoder.bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(decoder.needed().is_err());
        decoder.bytes = frame(1, &[1]);
        decoder.bytes[8..10].copy_from_slice(&(VERSION + 1).to_le_bytes());
        assert!(decoder.take().is_err());
        assert!(
            validate_progress(
                None,
                Progress {
                    hits: u64::MAX,
                    misses: 1,
                    combo: 0,
                    max_combo: 0,
                    song_ns: 0
                }
            )
            .is_err()
        );
        assert!(
            validate_progress(
                None,
                Progress {
                    combo: 3,
                    ..progress()
                }
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod final_prefix_fixtures {
    use super::*;
    #[test]
    fn readiness_requires_both_receipt_and_complete_local_frame() {
        let mut left = Protocol::default();
        let mut right = Protocol::default();
        assert!(left.next_ready(false).is_none());
        assert!(!left.ready());
        let left_frame = left.next_ready(true).unwrap();
        assert!(left.next_ready(true).is_none());
        let (tag, payload) = decode(&left_frame);
        assert_eq!(tag, 5);
        assert!(payload.is_empty());
        right.receive(tag, &payload).unwrap();
        assert!(right.readiness().is_none());
        let right_frame = right.next_ready(true).unwrap();
        let (tag, payload) = decode(&right_frame);
        left.receive(tag, &payload).unwrap();
        assert!(left.readiness().is_none()); // a partial local write is not readiness
        left.written(5);
        assert_eq!(left.readiness(), Some(MultiplayerEvent::Ready));
        assert!(left.readiness().is_none());
        assert!(right.readiness().is_none());
        right.written(5);
        assert_eq!(right.readiness(), Some(MultiplayerEvent::Ready));
        assert!(right.readiness().is_none());
        assert!(left.outgoing(message(1, false)).is_ok());
    }
    #[test]
    fn readiness_payload_duplicates_and_early_data_reject_atomically() {
        let mut state = Protocol::default();
        assert!(state.receive(5, &[0]).is_err());
        assert!(!state.remote_ready);
        for tag in [2, 3] {
            let payload = decode(&prefix_frame(tag, 0, progress(1))).1;
            assert!(state.receive(tag, &payload).is_err());
            assert_eq!(state.rx_sequence, 0);
            assert!(state.remote.is_none() && !state.remote_final && state.pending_ack.is_none());
        }
        assert!(state.outgoing(message(1, true)).is_err());
        assert!(state.local_final.is_none() && state.tx_sequence == 0);
        state.receive(5, &[]).unwrap();
        assert!(state.receive(5, &[]).is_err());
        assert!(!state.ready());
        state.next_ready(true).unwrap();
        state.written(5);
        assert_eq!(state.readiness(), Some(MultiplayerEvent::Ready));
        for version in [1u16, 2, 3, 4, 5] {
            let mut frames = Frames::new();
            frames.bytes = frame(5, &[]);
            frames.bytes[8..10].copy_from_slice(&version.to_le_bytes());
            assert!(frames.take().is_err());
        }
    }
    #[test]
    fn owner_ready_request_is_separate_one_shot_and_cancel_fenced() {
        let (mut owner, outgoing, incoming, _terminal) = owner();
        owner.try_publish(progress(1)).unwrap(); // occupy the one data slot
        owner.ready = false;
        owner.start_schedule = None;
        owner.ready_requested.store(false, Ordering::Release);
        assert!(!owner.is_ready());
        assert!(owner.try_publish(progress(2)).is_err());
        assert!(owner.try_finish(progress(2)).is_err());
        owner.try_ready().unwrap();
        assert!(owner.ready_requested.load(Ordering::Acquire));
        assert!(owner.try_ready().is_err());
        assert!(!owner.is_ready());
        assert_eq!(
            scalar_outgoing(outgoing.try_recv().unwrap()).progress,
            progress(1)
        );
        incoming
            .try_send(MultiplayerNotice::Session(MultiplayerEvent::Ready))
            .unwrap();
        assert_eq!(owner.poll(), vec![MultiplayerEvent::Ready]);
        assert!(owner.is_ready());
        assert!(owner.try_publish(progress(2)).is_err());
        let mut filter = ClockFilter::new();
        filter
            .observe(ClockSample::new(100, 160, 180, 140).unwrap())
            .unwrap();
        let estimate = filter.estimate().unwrap();
        incoming
            .try_send(MultiplayerNotice::Session(
                MultiplayerEvent::ClockEstimated(estimate),
            ))
            .unwrap();
        assert_eq!(
            owner.poll(),
            vec![MultiplayerEvent::ClockEstimated(estimate)]
        );
        assert_eq!(owner.clock_estimate(), Some(estimate));
        assert!(owner.try_finish(progress(2)).is_err());
        let schedule = StartSchedule {
            target_ns: 1_000,
            song_target_ns: 1_000,
            uncertainty_ns: 20,
        };
        incoming
            .try_send(MultiplayerNotice::Session(
                MultiplayerEvent::StartScheduled(schedule),
            ))
            .unwrap();
        assert_eq!(
            owner.poll(),
            vec![MultiplayerEvent::StartScheduled(schedule)]
        );
        assert_eq!(owner.start_schedule(), Some(schedule));
        assert!(owner.clock_now_ns().unwrap() >= 0);
        owner.request_stop();
        assert!(!owner.is_ready());
        assert_eq!(owner.clock_estimate(), Some(estimate));
        assert_eq!(owner.try_ready(), Err(MultiplayerError::Closed));
        assert_eq!(
            owner.try_publish(progress(2)),
            Err(MultiplayerError::Closed)
        );
    }
    fn playing() -> Protocol {
        Protocol {
            local_ready_sent: true,
            local_ready_written: true,
            remote_ready: true,
            ready_emitted: true,
            ..Protocol::default()
        }
    }
    fn progress(song_ns: i64) -> Progress {
        Progress {
            song_ns,
            hits: 1,
            misses: 0,
            combo: 1,
            max_combo: 1,
        }
    }
    fn message(song_ns: i64, final_prefix: bool) -> Outgoing {
        Outgoing {
            progress: progress(song_ns),
            final_prefix,
        }
    }
    fn decode(bytes: &[u8]) -> (u8, Vec<u8>) {
        let mut frames = Frames::new();
        for byte in bytes {
            frames.bytes.push(*byte);
            if frames.bytes.len() < bytes.len() {
                assert!(frames.take().unwrap().is_none());
            }
        }
        frames.take().unwrap().unwrap()
    }
    #[test]
    fn fragmented_final_shares_sequence_and_retains_terminal_prefix() {
        let mut sender = playing();
        let mut peer = playing();
        let (tag, payload) = decode(&sender.outgoing(message(1, false)).unwrap());
        assert_eq!(
            peer.receive(tag, &payload).unwrap(),
            Some(MultiplayerEvent::Progress(progress(1)))
        );
        let (tag, payload) = decode(&sender.outgoing(message(2, true)).unwrap());
        assert_eq!(tag, 3);
        assert_eq!(&payload[..8], &1u64.to_le_bytes());
        assert_eq!(
            peer.receive(tag, &payload).unwrap(),
            Some(MultiplayerEvent::FinalProgress(progress(2)))
        );
        assert_eq!(peer.remote, Some(progress(2)));
        assert_eq!(peer.pending_ack, Some(1));
        assert!(sender.outgoing(message(3, false)).is_err());
        assert!(peer.receive(tag, &payload).is_err());
        assert!(
            peer.receive(2, &decode(&progress_frame(2, progress(3))).1)
                .is_err()
        );
    }
    #[test]
    fn simultaneous_finals_require_complete_peer_ack_write_before_notification() {
        let mut left = playing();
        let mut right = playing();
        let left_frame = left.outgoing(message(1, true)).unwrap();
        let right_frame = right.outgoing(message(1, true)).unwrap();
        left.written(3);
        right.written(3);
        let (tag, payload) = decode(&right_frame);
        left.receive(tag, &payload).unwrap();
        let (tag, payload) = decode(&left_frame);
        right.receive(tag, &payload).unwrap();
        let left_ack = left.next_ack().unwrap();
        let right_ack = right.next_ack().unwrap();
        let (tag, payload) = decode(&right_ack);
        left.receive(tag, &payload).unwrap();
        assert!(left.acknowledgement().is_none()); // own ack only partially written
        left.written(4);
        assert_eq!(
            left.acknowledgement(),
            Some(MultiplayerEvent::FinalAcknowledged)
        );
        assert!(left.acknowledgement().is_none());
        let (tag, payload) = decode(&left_ack);
        right.receive(tag, &payload).unwrap();
        assert!(right.acknowledgement().is_none());
        right.written(4);
        assert_eq!(
            right.acknowledgement(),
            Some(MultiplayerEvent::FinalAcknowledged)
        );
        assert!(left.receive(4, &0u64.to_le_bytes()).is_err());
    }
    #[test]
    fn malformed_unsolicited_wrong_and_early_ack_preserve_state() {
        let mut state = playing();
        assert!(state.receive(4, &0u64.to_le_bytes()).is_err());
        state.outgoing(message(1, true)).unwrap();
        assert!(state.receive(4, &0u64.to_le_bytes()).is_err()); // not written yet
        state.written(3);
        for payload in [vec![], vec![0; 7], 1u64.to_le_bytes().to_vec()] {
            assert!(state.receive(4, &payload).is_err());
        }
        assert!(!state.local_ack_received);
        let mut invalid = decode(&prefix_frame(3, 1, progress(1))).1;
        assert!(state.receive(3, &invalid).is_err());
        assert_eq!(state.rx_sequence, 0);
        invalid[..8].copy_from_slice(&0u64.to_le_bytes());
        invalid[32..40].copy_from_slice(&2u64.to_le_bytes()); // combo greater than max
        assert!(state.receive(3, &invalid).is_err());
        assert!(!state.remote_final && state.pending_ack.is_none());
        state.receive(4, &0u64.to_le_bytes()).unwrap();
        assert!(state.local_ack_received);
    }
    #[test]
    fn version_six_wire_and_coalesced_complete_frames() {
        let mut wire = progress_frame(0, progress(1));
        wire.extend_from_slice(&prefix_frame(3, 1, progress(2)));
        let mut frames = Frames::new();
        let mut state = playing();
        let mut cursor = 0;
        while cursor < wire.len() {
            let count = frames.needed().unwrap().min(wire.len() - cursor);
            frames
                .bytes
                .extend_from_slice(&wire[cursor..cursor + count]);
            cursor += count;
            if let Some((tag, payload)) = frames.take().unwrap() {
                state.receive(tag, &payload).unwrap();
            }
        }
        assert!(state.remote_final);
        assert_eq!(state.remote, Some(progress(2)));
        let mut old = progress_frame(0, progress(1));
        old[8..10].copy_from_slice(&1u16.to_le_bytes());
        let mut frames = Frames::new();
        frames.bytes = old;
        assert!(frames.take().is_err());
        let mut exhausted = Protocol {
            tx_sequence: u64::MAX,
            ..playing()
        };
        assert!(exhausted.outgoing(message(1, true)).is_err());
        assert!(exhausted.local_final.is_none() && exhausted.local.is_none());
    }
    fn scalar_outgoing(message: OutgoingMessage) -> Outgoing {
        match message {
            OutgoingMessage::Scalar(outgoing) => outgoing,
            OutgoingMessage::Group { .. } => panic!("scalar fixture received group data"),
        }
    }
    fn owner() -> (
        Multiplayer,
        Receiver<OutgoingMessage>,
        SyncSender<MultiplayerNotice>,
        SyncSender<MultiplayerError>,
    ) {
        let (outgoing, out_rx) = mpsc::sync_channel(1);
        let (in_tx, incoming) = mpsc::sync_channel(8);
        let (terminal_tx, terminal) = mpsc::sync_channel(1);
        (
            Multiplayer {
                outgoing,
                incoming,
                terminal,
                stop_flag: Arc::new(AtomicBool::new(false)),
                ready_requested: Arc::new(AtomicBool::new(true)),
                ready: true,
                clock_epoch: Instant::now(),
                clock_estimate: None,
                start_schedule: Some(StartSchedule {
                    target_ns: 1_000,
                    song_target_ns: 1_000,
                    uncertainty_ns: 0,
                }),
                start_policy: StartPolicy::default(),
                worker: None,
                local: None,
                remote: None,
                closed: false,
                connected: true,
                local_final: false,
                remote_final: None,
                final_acknowledged: false,
                finish_timeout: Duration::ZERO,
                group: None,
            },
            out_rx,
            in_tx,
            terminal_tx,
        )
    }
    #[test]
    fn admission_full_is_atomic_then_terminal_is_immutable_and_cancel_is_immediate() {
        let (mut owner, outgoing, _incoming, _terminal) = owner();
        owner.try_publish(progress(1)).unwrap();
        assert_eq!(
            owner.try_finish(progress(2)),
            Err(MultiplayerError::QueueFull)
        );
        assert!(!owner.local_final && !owner.closed);
        assert_eq!(owner.local, Some(progress(1)));
        assert!(!scalar_outgoing(outgoing.try_recv().unwrap()).final_prefix);
        owner.try_finish(progress(2)).unwrap();
        assert!(scalar_outgoing(outgoing.try_recv().unwrap()).final_prefix);
        assert!(owner.try_publish(progress(3)).is_err());
        assert!(owner.try_finish(progress(3)).is_err());
        owner.request_stop();
        assert_eq!(owner.try_finish(progress(3)), Err(MultiplayerError::Closed));
    }
    #[test]
    fn confirmed_delivery_survives_later_eof_but_not_protocol_failure() {
        for (reason, expected) in [
            (MultiplayerError::Closed, Ok(())),
            (
                MultiplayerError::Protocol("malformed later frame".into()),
                Err(MultiplayerError::Protocol("malformed later frame".into())),
            ),
            (
                MultiplayerError::WorkerPanicked,
                Err(MultiplayerError::WorkerPanicked),
            ),
        ] {
            let (mut owner, outgoing, incoming, terminal) = owner();
            owner.finish_timeout = Duration::from_secs(1);
            owner.try_finish(progress(1)).unwrap();
            assert!(scalar_outgoing(outgoing.try_recv().unwrap()).final_prefix);
            incoming
                .try_send(MultiplayerNotice::Session(MultiplayerEvent::FinalProgress(
                    progress(2),
                )))
                .unwrap();
            incoming
                .try_send(MultiplayerNotice::Session(
                    MultiplayerEvent::FinalAcknowledged,
                ))
                .unwrap();
            terminal.try_send(reason).unwrap();
            let deadline = Instant::now() + owner.finish_timeout;
            assert_eq!(owner.wait_for_delivery(progress(1), deadline), expected);
            assert_eq!(owner.remote_final_progress(), Some(progress(2)));
            assert!(owner.final_acknowledged);
        }
    }
    #[test]
    fn cleanup_wait_timeout_and_remote_final_retention_need_no_transport() {
        let (mut owner, _outgoing, incoming, _terminal) = owner();
        assert_eq!(
            owner.finish_delivery(progress(1)),
            Err(MultiplayerError::IoStalled)
        );
        assert!(!owner.local_final);
        incoming
            .try_send(MultiplayerNotice::Session(MultiplayerEvent::FinalProgress(
                progress(2),
            )))
            .unwrap();
        assert_eq!(
            owner.poll(),
            vec![MultiplayerEvent::FinalProgress(progress(2))]
        );
        assert_eq!(owner.remote_progress(), Some(progress(2)));
        assert_eq!(owner.remote_final_progress(), Some(progress(2)));
        owner.request_stop();
        assert_eq!(
            owner.finish_delivery(progress(2)),
            Err(MultiplayerError::Closed)
        );
        assert_eq!(owner.remote_final_progress(), Some(progress(2)));
    }
}
