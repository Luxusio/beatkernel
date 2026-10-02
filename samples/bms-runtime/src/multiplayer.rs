//! Casual two-player progress transport. Remote scores are unauthenticated display data.
//! Socket work never runs on the gameplay or audio thread. Software starts are committed.
use crate::multiplayer_clock::{ClockFilter, ClockSample, OffsetEstimate};
use crate::multiplayer_start::{
    StartAgreement, StartMessage, StartPolicy, StartRole, StartSchedule,
};
use beatkernel::{
    replay::{
        ReplayHeader,
        codec::{ReplayCodecLimits, ReplayFile, encode_replay},
    },
    time::{ClockDomainId, Timestamp},
};
use std::{
    fmt,
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAGIC: &[u8; 4] = b"BKMP";
const VERSION: u16 = 5;
const CLOCK_PROBES: u64 = 8;
const MAX_IDENTITY: usize = 65_536;
const MAX_BODY: usize = MAX_IDENTITY + 7;
const TICK: Duration = Duration::from_millis(5);

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
    let (_, start, _) = crate::replay_playback::decode_chart_setup(&header.options)
        .map_err(|error| MultiplayerError::Protocol(error.to_string()))?;
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

/// A local cumulative summary; no implicit grade weighting or ranked-score claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub song_ns: i64,
    pub hits: u64,
    pub misses: u64,
    pub combo: u64,
    pub max_combo: u64,
}

#[derive(Clone, Debug)]
pub struct MultiplayerOptions {
    /// Includes connect, identity, readiness and clock probes; 1 ms through 120 seconds.
    pub setup_timeout: Duration,
    /// Each application queue has this capacity, 1 through 1,024 messages.
    pub queue_capacity: usize,
    /// Maximum lack of I/O progress while a frame is pending; quiet peers stay connected.
    pub io_stall_timeout: Duration,
    /// Checked software-start lead, clock age and uncertainty bounds.
    pub start_policy: StartPolicy,
}
impl Default for MultiplayerOptions {
    fn default() -> Self {
        Self {
            setup_timeout: Duration::from_secs(10),
            queue_capacity: 32,
            io_stall_timeout: Duration::from_secs(5),
            start_policy: StartPolicy::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MultiplayerError {
    InvalidOptions,
    Io(String),
    SetupTimeout,
    IoStalled,
    IncompatibleSetup,
    Protocol(String),
    QueueFull,
    Closed,
    WorkerPanicked,
}
impl fmt::Display for MultiplayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for MultiplayerError {}
impl From<io::Error> for MultiplayerError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MultiplayerEvent {
    Connected,
    /// Compatible peers both completely sent and received preparation readiness.
    Ready,
    /// Minimum-delay software offset interval after eight validated probes.
    ClockEstimated(OffsetEstimate),
    /// A committed local session-clock target for the software start call.
    StartScheduled(StartSchedule),
    Progress(Progress),
    /// Peer terminal self-reported prefix, including aborted sessions.
    FinalProgress(Progress),
    /// Exact local final was acknowledged and any parsed peer ack was written.
    FinalAcknowledged,
    Disconnected(MultiplayerError),
}

/// Owned worker lifecycle. `stop` and Drop signal shutdown and join the worker.
/// Ordinary progress queue overflow terminates the session explicitly.
/// Terminal admission can retry capacity until the cleanup delivery deadline.
pub struct Multiplayer {
    outgoing: SyncSender<Outgoing>,
    incoming: Receiver<MultiplayerEvent>,
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
}
impl Multiplayer {
    /// Bind only the caller's explicit address. Binding errors are returned immediately.
    pub fn host(
        address: SocketAddr,
        identity: Vec<u8>,
        options: MultiplayerOptions,
    ) -> Result<Self, MultiplayerError> {
        validate_options(&identity, &options)?;
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        Self::spawn(Endpoint::Host(listener), identity, options)
    }
    pub fn join(
        address: SocketAddr,
        identity: Vec<u8>,
        options: MultiplayerOptions,
    ) -> Result<Self, MultiplayerError> {
        validate_options(&identity, &options)?;
        Self::spawn(Endpoint::Join(address), identity, options)
    }
    fn spawn(
        endpoint: Endpoint,
        identity: Vec<u8>,
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
                    identity,
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
        if self.closed || self.stop_flag.load(Ordering::Acquire) {
            return Err(MultiplayerError::Closed);
        }
        if !self.is_ready() || self.start_schedule.is_none() {
            return Err(MultiplayerError::Protocol(
                "committed start required".into(),
            ));
        }
        if self.local_final {
            return Err(MultiplayerError::Protocol(
                "local final already admitted".into(),
            ));
        }
        validate_progress(self.local, progress)?;
        match self.outgoing.try_send(Outgoing {
            progress,
            final_prefix: false,
        }) {
            Ok(()) => {
                self.local = Some(progress);
                if let Some(worker) = &self.worker {
                    worker.thread().unpark();
                }
                Ok(())
            }
            Err(TrySendError::Full(_)) => {
                self.signal_stop();
                self.closed = true;
                Err(MultiplayerError::QueueFull)
            }
            Err(TrySendError::Disconnected(_)) => {
                self.closed = true;
                Err(MultiplayerError::Closed)
            }
        }
    }
    /// Nonblocking terminal admission, immutable after success. Full is retryable.
    pub fn try_finish(&mut self, progress: Progress) -> Result<(), MultiplayerError> {
        if self.closed || self.stop_flag.load(Ordering::Acquire) {
            return Err(MultiplayerError::Closed);
        }
        if !self.is_ready() || self.start_schedule.is_none() {
            return Err(MultiplayerError::Protocol(
                "committed start required".into(),
            ));
        }
        if self.local_final {
            return Err(MultiplayerError::Protocol(
                "local final already admitted".into(),
            ));
        }
        validate_progress(self.local, progress)?;
        self.outgoing
            .try_send(Outgoing {
                progress,
                final_prefix: true,
            })
            .map_err(queue_error)?;
        self.local = Some(progress);
        self.local_final = true;
        if let Some(worker) = &self.worker {
            worker.thread().unpark();
        }
        Ok(())
    }
    /// Cleanup-only bounded final delivery; performs no socket I/O on the caller.
    /// Call after native resources are stopped. Immediate cancellation still wins.
    pub fn finish_delivery(&mut self, progress: Progress) -> Result<(), MultiplayerError> {
        if self.local_final {
            return Err(MultiplayerError::Protocol(
                "local final already admitted".into(),
            ));
        }
        validate_progress(self.local, progress)?;
        let deadline = Instant::now() + self.finish_timeout;
        self.wait_for_delivery(progress, deadline)
    }
    fn wait_for_delivery(
        &mut self,
        progress: Progress,
        deadline: Instant,
    ) -> Result<(), MultiplayerError> {
        let mut admitted = self.local_final;
        loop {
            let mut failure = None;
            for event in self.poll() {
                if let MultiplayerEvent::Disconnected(error) = event {
                    failure = Some(error);
                }
            }
            if self.stop_flag.load(Ordering::Acquire) {
                return Err(MultiplayerError::Closed);
            }
            if let Some(error) = failure {
                if error != MultiplayerError::Closed || !self.final_acknowledged {
                    return Err(error);
                }
            }
            if self.final_acknowledged {
                return Ok(());
            }
            if self.closed {
                return Err(MultiplayerError::Closed);
            }
            if Instant::now() >= deadline {
                return Err(MultiplayerError::IoStalled);
            }
            if !admitted {
                match self.try_finish(progress) {
                    Ok(()) => admitted = true,
                    Err(MultiplayerError::QueueFull) => {}
                    Err(error) => return Err(error),
                }
            }
            thread::park_timeout(TICK.min(deadline.saturating_duration_since(Instant::now())));
        }
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
    /// Drain a bounded batch; terminal notification follows all retained remote data.
    pub fn poll(&mut self) -> Vec<MultiplayerEvent> {
        // Worker may keep producing, so retain an explicit per-call bound.
        let mut events = Vec::new();
        for _ in 0..1024 {
            match self.incoming.try_recv() {
                Ok(event) => {
                    self.retain_event(&event);
                    events.push(event);
                }
                Err(_) => break,
            }
        }
        let terminal = match self.terminal.try_recv() {
            Ok(reason) => Some(reason),
            Err(TryRecvError::Disconnected) if !self.closed => {
                Some(MultiplayerError::WorkerPanicked)
            }
            Err(_) => None,
        };
        if let Some(reason) = terminal {
            // The worker has exited before publishing its terminal slot. Drain its
            // remaining bounded queue so no progress is emitted after disconnect.
            for _ in 0..1024 {
                match self.incoming.try_recv() {
                    Ok(event) => {
                        self.retain_event(&event);
                        events.push(event);
                    }
                    Err(_) => break,
                }
            }
            self.closed = true;
            self.connected = false;
            events.push(MultiplayerEvent::Disconnected(reason));
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
    /// Joining waits for at most the current bounded connect attempt plus worker scheduling.
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
impl Drop for Multiplayer {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

enum Endpoint {
    Host(TcpListener),
    Join(SocketAddr),
}
fn validate_identity(identity: &[u8]) -> Result<(), MultiplayerError> {
    if identity.is_empty() || identity.len() > MAX_IDENTITY {
        return Err(MultiplayerError::InvalidOptions);
    }
    Ok(())
}
fn validate_options(identity: &[u8], options: &MultiplayerOptions) -> Result<(), MultiplayerError> {
    validate_identity(identity)?;
    options
        .start_policy
        .validate()
        .map_err(|_| MultiplayerError::InvalidOptions)?;
    if options.setup_timeout < Duration::from_millis(1)
        || options.setup_timeout > Duration::from_secs(120)
        || options.io_stall_timeout < Duration::from_millis(1)
        || options.io_stall_timeout > Duration::from_secs(60)
        || !(1..=1024).contains(&options.queue_capacity)
    {
        return Err(MultiplayerError::InvalidOptions);
    }
    Ok(())
}
fn validate_progress(previous: Option<Progress>, next: Progress) -> Result<(), MultiplayerError> {
    if next.combo > next.max_combo
        || next.max_combo > next.hits
        || next.hits.checked_add(next.misses).is_none()
    {
        return Err(MultiplayerError::Protocol(
            "invalid cumulative counts".into(),
        ));
    }
    if let Some(previous) = previous {
        if next.song_ns < previous.song_ns
            || next.hits < previous.hits
            || next.misses < previous.misses
            || next.max_combo < previous.max_combo
        {
            return Err(MultiplayerError::Protocol(
                "progress regression; reconnect after section restart".into(),
            ));
        }
        let added_hits = next.hits - previous.hits;
        if next.combo > previous.combo.saturating_add(added_hits)
            || next.max_combo
                > previous
                    .max_combo
                    .max(previous.combo.saturating_add(added_hits))
            || (next.misses == previous.misses
                && (next.combo != previous.combo + added_hits
                    || next.max_combo != previous.max_combo.max(next.combo)))
        {
            return Err(MultiplayerError::Protocol(
                "inconsistent combo transition".into(),
            ));
        }
    }
    Ok(())
}
fn frame(tag: u8, payload: &[u8]) -> Vec<u8> {
    let len = 7 + payload.len();
    let mut bytes = Vec::with_capacity(4 + len);
    bytes.extend_from_slice(&(len as u32).to_le_bytes());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.push(tag);
    bytes.extend_from_slice(payload);
    bytes
}
fn progress_frame(sequence: u64, progress: Progress) -> Vec<u8> {
    prefix_frame(2, sequence, progress)
}
fn prefix_frame(tag: u8, sequence: u64, progress: Progress) -> Vec<u8> {
    let mut payload = Vec::with_capacity(48);
    payload.extend_from_slice(&sequence.to_le_bytes());
    payload.extend_from_slice(&progress.song_ns.to_le_bytes());
    for value in [
        progress.hits,
        progress.misses,
        progress.combo,
        progress.max_combo,
    ] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    frame(tag, &payload)
}
fn parse_progress(
    payload: &[u8],
    expected: u64,
    previous: Option<Progress>,
) -> Result<Progress, MultiplayerError> {
    if payload.len() != 48 {
        return Err(MultiplayerError::Protocol("invalid progress size".into()));
    }
    let word = |offset: usize| u64::from_le_bytes(payload[offset..offset + 8].try_into().unwrap());
    if word(0) != expected {
        return Err(MultiplayerError::Protocol("invalid sequence".into()));
    }
    let progress = Progress {
        song_ns: i64::from_le_bytes(payload[8..16].try_into().unwrap()),
        hits: word(16),
        misses: word(24),
        combo: word(32),
        max_combo: word(40),
    };
    validate_progress(previous, progress)?;
    Ok(progress)
}

fn elapsed_ns(epoch: Instant) -> Result<i64, MultiplayerError> {
    i64::try_from(epoch.elapsed().as_nanos())
        .map_err(|_| MultiplayerError::Protocol("session clock extent exceeded".into()))
}

fn start_frame(message: StartMessage) -> Vec<u8> {
    match message {
        StartMessage::ClockReady => frame(8, &[]),
        StartMessage::Propose(time) => frame(9, &time.to_le_bytes()),
        StartMessage::Accept(time) => frame(10, &time.to_le_bytes()),
        StartMessage::Commit(time) => frame(11, &time.to_le_bytes()),
    }
}
fn parse_start_frame(tag: u8, payload: &[u8]) -> Result<StartMessage, MultiplayerError> {
    if tag == 8 && payload.is_empty() {
        return Ok(StartMessage::ClockReady);
    }
    if !(9..=11).contains(&tag) || payload.len() != 8 {
        return Err(MultiplayerError::Protocol("invalid start frame".into()));
    }
    let time = i64::from_le_bytes(payload.try_into().unwrap());
    Ok(match tag {
        9 => StartMessage::Propose(time),
        10 => StartMessage::Accept(time),
        _ => StartMessage::Commit(time),
    })
}
fn start_error(error: impl fmt::Display) -> MultiplayerError {
    MultiplayerError::Protocol(error.to_string())
}

/// Finite symmetric software probes; only the worker reads the actual clock.
#[derive(Default)]
struct ClockProbes {
    completed: u64,
    pending_ping: Option<(u64, i64)>,
    peer_sequence: u64,
    peer_send: Option<i64>,
    pending_pong: Option<(u64, i64, i64)>,
    last_receive: i64,
    filter: ClockFilter,
    emitted: bool,
}
impl ClockProbes {
    fn next_ping(&mut self, now: i64) -> Result<Option<Vec<u8>>, MultiplayerError> {
        if self.pending_ping.is_some() || self.completed == CLOCK_PROBES {
            return Ok(None);
        }
        if now < self.last_receive {
            return Err(MultiplayerError::Protocol("probe clock regressed".into()));
        }
        let mut payload = Vec::with_capacity(16);
        payload.extend_from_slice(&self.completed.to_le_bytes());
        payload.extend_from_slice(&now.to_le_bytes());
        self.pending_ping = Some((self.completed, now));
        Ok(Some(frame(6, &payload)))
    }
    fn receive_ping(&mut self, payload: &[u8], now: i64) -> Result<(), MultiplayerError> {
        if payload.len() != 16 {
            return Err(MultiplayerError::Protocol("invalid clock ping size".into()));
        }
        let sequence = u64::from_le_bytes(payload[..8].try_into().unwrap());
        let sent = i64::from_le_bytes(payload[8..].try_into().unwrap());
        if sequence != self.peer_sequence
            || sequence >= CLOCK_PROBES
            || sent < 0
            || now < 0
            || self.peer_send.is_some_and(|previous| sent < previous)
            || self.pending_pong.is_some()
        {
            return Err(MultiplayerError::Protocol(
                "invalid clock ping sequence/time".into(),
            ));
        }
        self.pending_pong = Some((sequence, sent, now));
        self.peer_sequence += 1;
        self.peer_send = Some(sent);
        Ok(())
    }
    fn next_pong(&mut self, now: i64) -> Result<Option<Vec<u8>>, MultiplayerError> {
        let Some((sequence, echo, received)) = self.pending_pong else {
            return Ok(None);
        };
        if now < received {
            return Err(MultiplayerError::Protocol(
                "clock pong send precedes receipt".into(),
            ));
        }
        let mut payload = Vec::with_capacity(32);
        payload.extend_from_slice(&sequence.to_le_bytes());
        for timestamp in [echo, received, now] {
            payload.extend_from_slice(&timestamp.to_le_bytes());
        }
        self.pending_pong = None;
        Ok(Some(frame(7, &payload)))
    }
    fn receive_pong(&mut self, payload: &[u8], now: i64) -> Result<(), MultiplayerError> {
        if payload.len() != 32 {
            return Err(MultiplayerError::Protocol("invalid clock pong size".into()));
        }
        let word =
            |offset: usize| i64::from_le_bytes(payload[offset..offset + 8].try_into().unwrap());
        let sequence = u64::from_le_bytes(payload[..8].try_into().unwrap());
        let Some((expected, sent)) = self.pending_ping else {
            return Err(MultiplayerError::Protocol("unsolicited clock pong".into()));
        };
        if sequence != expected || word(8) != sent {
            return Err(MultiplayerError::Protocol(
                "clock pong sequence/echo mismatch".into(),
            ));
        }
        let sample = ClockSample::new(sent, word(16), word(24), now)
            .map_err(|error| MultiplayerError::Protocol(error.to_string()))?;
        self.filter
            .observe(sample)
            .map_err(|error| MultiplayerError::Protocol(error.to_string()))?;
        self.completed += 1;
        self.last_receive = now;
        self.pending_ping = None;
        Ok(())
    }
    fn estimate_event(&mut self) -> Option<MultiplayerEvent> {
        if self.completed != CLOCK_PROBES || self.emitted {
            return None;
        }
        let estimate = self.filter.estimate()?;
        self.emitted = true;
        Some(MultiplayerEvent::ClockEstimated(estimate))
    }
}

#[derive(Clone, Copy)]
struct Outgoing {
    progress: Progress,
    final_prefix: bool,
}
#[derive(Default)]
struct Protocol {
    local_ready_sent: bool,
    local_ready_written: bool,
    remote_ready: bool,
    ready_emitted: bool,
    tx_sequence: u64,
    rx_sequence: u64,
    local: Option<Progress>,
    remote: Option<Progress>,
    local_final: Option<u64>,
    local_final_written: bool,
    remote_final: bool,
    pending_ack: Option<u64>,
    ack_in_flight: bool,
    local_ack_received: bool,
    acknowledgement_emitted: bool,
}
impl Protocol {
    fn ready(&self) -> bool {
        self.local_ready_written && self.remote_ready
    }
    fn next_ready(&mut self, requested: bool) -> Option<Vec<u8>> {
        if !requested || self.local_ready_sent {
            return None;
        }
        self.local_ready_sent = true;
        Some(frame(5, &[]))
    }
    fn readiness(&mut self) -> Option<MultiplayerEvent> {
        if self.ready() && !self.ready_emitted {
            self.ready_emitted = true;
            Some(MultiplayerEvent::Ready)
        } else {
            None
        }
    }
    fn outgoing(&mut self, message: Outgoing) -> Result<Vec<u8>, MultiplayerError> {
        if !self.ready() {
            return Err(MultiplayerError::Protocol(
                "bilateral readiness required".into(),
            ));
        }
        if self.local_final.is_some() {
            return Err(MultiplayerError::Protocol(
                "progress after local final".into(),
            ));
        }
        validate_progress(self.local, message.progress)?;
        let next = self
            .tx_sequence
            .checked_add(1)
            .ok_or_else(|| MultiplayerError::Protocol("sequence exhausted".into()))?;
        let bytes = if message.final_prefix {
            prefix_frame(3, self.tx_sequence, message.progress)
        } else {
            progress_frame(self.tx_sequence, message.progress)
        };
        if message.final_prefix {
            self.local_final = Some(self.tx_sequence);
        }
        self.tx_sequence = next;
        self.local = Some(message.progress);
        Ok(bytes)
    }
    fn receive(
        &mut self,
        tag: u8,
        payload: &[u8],
    ) -> Result<Option<MultiplayerEvent>, MultiplayerError> {
        if tag != 5 && !self.ready() {
            return Err(MultiplayerError::Protocol(
                "data before bilateral readiness".into(),
            ));
        }
        match tag {
            5 => {
                if !payload.is_empty() || self.remote_ready {
                    return Err(MultiplayerError::Protocol(
                        "invalid or duplicate readiness".into(),
                    ));
                }
                self.remote_ready = true;
                Ok(None)
            }
            2 | 3 => {
                if self.remote_final {
                    return Err(MultiplayerError::Protocol(
                        "progress after peer final".into(),
                    ));
                }
                let progress = parse_progress(payload, self.rx_sequence, self.remote)?;
                let next = self
                    .rx_sequence
                    .checked_add(1)
                    .ok_or_else(|| MultiplayerError::Protocol("sequence exhausted".into()))?;
                if tag == 3 {
                    self.pending_ack = Some(self.rx_sequence);
                    self.remote_final = true;
                }
                self.rx_sequence = next;
                self.remote = Some(progress);
                Ok(Some(if tag == 3 {
                    MultiplayerEvent::FinalProgress(progress)
                } else {
                    MultiplayerEvent::Progress(progress)
                }))
            }
            4 => {
                if payload.len() != 8 {
                    return Err(MultiplayerError::Protocol(
                        "invalid acknowledgement size".into(),
                    ));
                }
                let sequence = u64::from_le_bytes(payload.try_into().unwrap());
                if self.local_final != Some(sequence)
                    || !self.local_final_written
                    || self.local_ack_received
                {
                    return Err(MultiplayerError::Protocol(
                        "unsolicited, duplicate or wrong acknowledgement".into(),
                    ));
                }
                self.local_ack_received = true;
                Ok(None)
            }
            _ => Err(MultiplayerError::Protocol("unexpected message".into())),
        }
    }
    // Called only between complete frames: acknowledgements never interrupt payloads.
    fn next_ack(&mut self) -> Option<Vec<u8>> {
        let sequence = self.pending_ack.take()?;
        self.ack_in_flight = true;
        Some(frame(4, &sequence.to_le_bytes()))
    }
    fn written(&mut self, tag: u8) {
        if tag == 5 {
            self.local_ready_written = true;
        }
        if tag == 3 {
            self.local_final_written = true;
        }
        if tag == 4 {
            self.ack_in_flight = false;
        }
    }
    fn acknowledgement(&mut self) -> Option<MultiplayerEvent> {
        if self.local_ack_received
            && self.pending_ack.is_none()
            && !self.ack_in_flight
            && !self.acknowledgement_emitted
        {
            self.acknowledgement_emitted = true;
            Some(MultiplayerEvent::FinalAcknowledged)
        } else {
            None
        }
    }
}

/// Incremental decoder preserves partial headers/bodies and bounds allocation before reading.
struct Frames {
    bytes: Vec<u8>,
}
impl Frames {
    fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(MAX_BODY + 4),
        }
    }
    fn needed(&self) -> Result<usize, MultiplayerError> {
        if self.bytes.len() < 4 {
            return Ok(4 - self.bytes.len());
        }
        let length = u32::from_le_bytes(self.bytes[..4].try_into().unwrap()) as usize;
        if !(7..=MAX_BODY).contains(&length) {
            return Err(MultiplayerError::Protocol("invalid frame length".into()));
        }
        Ok(length + 4 - self.bytes.len())
    }
    fn take(&mut self) -> Result<Option<(u8, Vec<u8>)>, MultiplayerError> {
        if self.needed()? != 0 {
            return Ok(None);
        }
        if &self.bytes[4..8] != MAGIC
            || u16::from_le_bytes(self.bytes[8..10].try_into().unwrap()) != VERSION
        {
            return Err(MultiplayerError::Protocol(
                "unknown protocol/version".into(),
            ));
        }
        let tag = self.bytes[10];
        let payload = self.bytes[11..].to_vec();
        self.bytes.clear();
        Ok(Some((tag, payload)))
    }
}
fn run(
    endpoint: Endpoint,
    identity: Vec<u8>,
    options: MultiplayerOptions,
    stop: &AtomicBool,
    ready_requested: &AtomicBool,
    clock_epoch: Instant,
    outgoing: Receiver<Outgoing>,
    incoming: SyncSender<MultiplayerEvent>,
) -> Result<(), MultiplayerError> {
    let role = match &endpoint {
        Endpoint::Host(_) => StartRole::Host,
        Endpoint::Join(_) => StartRole::Join,
    };
    let mut start = StartAgreement::new(role, options.start_policy).map_err(start_error)?;
    let mut start_in_flight = None;
    let deadline = Instant::now() + options.setup_timeout;
    let mut stream = loop {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(MultiplayerError::SetupTimeout);
        }
        let result = match &endpoint {
            Endpoint::Host(listener) => listener.accept().map(|(stream, _)| stream),
            Endpoint::Join(address) => TcpStream::connect_timeout(
                address,
                Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
            ),
        };
        match result {
            Ok(stream) => break stream,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::ConnectionRefused
                        | io::ErrorKind::Interrupted
                ) =>
            {
                thread::park_timeout(TICK)
            }
            Err(error) => return Err(error.into()),
        }
    };
    stream.set_nonblocking(true)?;
    stream.set_nodelay(true)?;
    let mut tx = frame(1, &identity);
    let mut written = 0;
    let mut rx = Frames::new();
    let mut buffer = [0; 4096];
    let mut last_write = Instant::now();
    let mut last_read = Instant::now();
    let mut connected = false;
    let mut protocol = Protocol::default();
    let mut clocks = ClockProbes::default();
    let mut tx_tag = 1u8;
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        if (!connected
            || !protocol.ready()
            || clocks.completed < CLOCK_PROBES
            || !start.committed())
            && Instant::now() >= deadline
        {
            return Err(MultiplayerError::SetupTimeout);
        }
        if connected
            && ((written < tx.len() && last_write.elapsed() >= options.io_stall_timeout)
                || (!rx.bytes.is_empty() && last_read.elapsed() >= options.io_stall_timeout))
        {
            return Err(MultiplayerError::IoStalled);
        }
        if written < tx.len() {
            match stream.write(&tx[written..]) {
                Ok(0) => return Err(MultiplayerError::Closed),
                Ok(count) => {
                    written += count;
                    last_write = Instant::now();
                    if written == tx.len() {
                        protocol.written(tx_tag);
                        if let Some(message) = start_in_flight.take() {
                            start
                                .written(message, elapsed_ns(clock_epoch)?)
                                .map_err(start_error)?;
                        }
                        if let Some(schedule) = start.take_schedule() {
                            incoming
                                .try_send(MultiplayerEvent::StartScheduled(schedule))
                                .map_err(queue_error)?;
                        }
                        if let Some(event) = protocol.readiness() {
                            incoming.try_send(event).map_err(queue_error)?;
                        }
                        // Peer may close immediately after receiving this ack.
                        // Publish confirmed delivery before the following read sees EOF.
                        if let Some(event) = protocol.acknowledgement() {
                            incoming.try_send(event).map_err(queue_error)?;
                        }
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
                    rx.bytes.extend_from_slice(&buffer[..count]);
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
            if !connected {
                if tag != 1 {
                    return Err(MultiplayerError::Protocol("expected setup".into()));
                }
                if payload != identity {
                    return Err(MultiplayerError::IncompatibleSetup);
                }
                connected = true;
                incoming
                    .try_send(MultiplayerEvent::Connected)
                    .map_err(queue_error)?;
            } else if tag == 6 || tag == 7 {
                if !protocol.ready() {
                    return Err(MultiplayerError::Protocol(
                        "clock probe before readiness".into(),
                    ));
                }
                let now = elapsed_ns(clock_epoch)?;
                if tag == 6 {
                    clocks.receive_ping(&payload, now)?;
                } else {
                    clocks.receive_pong(&payload, now)?;
                }
            } else if (8..=11).contains(&tag) {
                if !protocol.ready() {
                    return Err(MultiplayerError::Protocol(
                        "start before native readiness".into(),
                    ));
                }
                start
                    .receive(parse_start_frame(tag, &payload)?, elapsed_ns(clock_epoch)?)
                    .map_err(start_error)?;
            } else {
                if matches!(tag, 2 | 3) && !start.committed() {
                    return Err(MultiplayerError::Protocol(
                        "progress before committed start".into(),
                    ));
                }
                if let Some(event) = protocol.receive(tag, &payload)? {
                    incoming.try_send(event).map_err(queue_error)?;
                }
            }
        }
        if connected && written == tx.len() {
            if let Some(ready) = protocol.next_ready(ready_requested.load(Ordering::Acquire)) {
                tx = ready;
                tx_tag = 5;
                written = 0;
                last_write = Instant::now();
            } else if let Some(ack) = protocol.next_ack() {
                tx = ack;
                tx_tag = 4;
                written = 0;
                last_write = Instant::now();
            } else if protocol.ready() && clocks.pending_pong.is_some() {
                tx = clocks.next_pong(elapsed_ns(clock_epoch)?)?.ok_or_else(|| {
                    MultiplayerError::Protocol("missing pending clock pong".into())
                })?;
                tx_tag = 7;
                written = 0;
                last_write = Instant::now();
            } else if protocol.ready()
                && clocks.pending_ping.is_none()
                && clocks.completed < CLOCK_PROBES
            {
                tx = clocks
                    .next_ping(elapsed_ns(clock_epoch)?)?
                    .ok_or_else(|| MultiplayerError::Protocol("missing next clock ping".into()))?;
                tx_tag = 6;
                written = 0;
                last_write = Instant::now();
            } else if let Some(message) =
                start.next(elapsed_ns(clock_epoch)?).map_err(start_error)?
            {
                tx = start_frame(message);
                tx_tag = tx[10];
                start_in_flight = Some(message);
                written = 0;
                last_write = Instant::now();
            } else {
                match outgoing.try_recv() {
                    Ok(message) => {
                        tx_tag = if message.final_prefix { 3 } else { 2 };
                        tx = protocol.outgoing(message)?;
                        written = 0;
                        last_write = Instant::now();
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => return Ok(()),
                }
            }
        }
        if let Some(event) = protocol.readiness() {
            incoming.try_send(event).map_err(queue_error)?;
        }
        if let Some(event) = protocol.acknowledgement() {
            incoming.try_send(event).map_err(queue_error)?;
        }
        if let Some(event) = clocks.estimate_event() {
            if let MultiplayerEvent::ClockEstimated(estimate) = &event {
                start.prepare(*estimate).map_err(start_error)?;
            }
            incoming.try_send(event).map_err(queue_error)?;
        }
        if let Some(schedule) = start.take_schedule() {
            incoming
                .try_send(MultiplayerEvent::StartScheduled(schedule))
                .map_err(queue_error)?;
        }
        thread::park_timeout(TICK);
    }
}
fn queue_error<T>(error: TrySendError<T>) -> MultiplayerError {
    match error {
        TrySendError::Full(_) => MultiplayerError::QueueFull,
        TrySendError::Disconnected(_) => MultiplayerError::Closed,
    }
}

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
            StartMessage::ClockReady,
            StartMessage::Propose(i64::MAX),
            StartMessage::Accept(123),
            StartMessage::Commit(123),
        ] {
            let wire = start_frame(message);
            let payload = decode(&wire, wire[10]);
            assert_eq!(parse_start_frame(wire[10], &payload).unwrap(), message);
        }
        assert!(parse_start_frame(8, &[0]).is_err());
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
        for version in [1u16, 2, 3, 4] {
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
        assert_eq!(outgoing.try_recv().unwrap().progress, progress(1));
        incoming.try_send(MultiplayerEvent::Ready).unwrap();
        assert_eq!(owner.poll(), vec![MultiplayerEvent::Ready]);
        assert!(owner.is_ready());
        assert!(owner.try_publish(progress(2)).is_err());
        let mut filter = ClockFilter::new();
        filter
            .observe(ClockSample::new(100, 160, 180, 140).unwrap())
            .unwrap();
        let estimate = filter.estimate().unwrap();
        incoming
            .try_send(MultiplayerEvent::ClockEstimated(estimate))
            .unwrap();
        assert_eq!(
            owner.poll(),
            vec![MultiplayerEvent::ClockEstimated(estimate)]
        );
        assert_eq!(owner.clock_estimate(), Some(estimate));
        assert!(owner.try_finish(progress(2)).is_err());
        let schedule = StartSchedule {
            target_ns: 1_000,
            uncertainty_ns: 20,
        };
        incoming
            .try_send(MultiplayerEvent::StartScheduled(schedule))
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
    fn version_five_wire_and_coalesced_complete_frames() {
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
    fn owner() -> (
        Multiplayer,
        Receiver<Outgoing>,
        SyncSender<MultiplayerEvent>,
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
        assert!(!outgoing.try_recv().unwrap().final_prefix);
        owner.try_finish(progress(2)).unwrap();
        assert!(outgoing.try_recv().unwrap().final_prefix);
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
            assert!(outgoing.try_recv().unwrap().final_prefix);
            incoming
                .try_send(MultiplayerEvent::FinalProgress(progress(2)))
                .unwrap();
            incoming
                .try_send(MultiplayerEvent::FinalAcknowledged)
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
            .try_send(MultiplayerEvent::FinalProgress(progress(2)))
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
