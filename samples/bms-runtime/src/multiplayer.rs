//! Casual two-player progress transport. Remote scores are unauthenticated display data.
//! Socket work never runs on the gameplay or audio thread. Each peer starts locally.
use beatkernel::{
    replay::{
        codec::{encode_replay, ReplayCodecLimits, ReplayFile},
        ReplayHeader,
    },
    time::ClockDomainId,
};
use std::{
    fmt,
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAGIC: &[u8; 4] = b"BKMP";
const VERSION: u16 = 1;
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
    /// Includes accept/connect and exact identity exchange; 1 ms through 120 seconds.
    pub setup_timeout: Duration,
    /// Each application queue has this capacity, 1 through 1,024 messages.
    pub queue_capacity: usize,
    /// Maximum lack of I/O progress while a frame is pending; quiet peers stay connected.
    pub io_stall_timeout: Duration,
}
impl Default for MultiplayerOptions {
    fn default() -> Self {
        Self {
            setup_timeout: Duration::from_secs(10),
            queue_capacity: 32,
            io_stall_timeout: Duration::from_secs(5),
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
    Progress(Progress),
    Disconnected(MultiplayerError),
}

/// Owned worker lifecycle. `stop` and Drop signal shutdown and join the worker.
/// Full queues terminate the session explicitly; snapshots are never silently lost.
pub struct Multiplayer {
    outgoing: SyncSender<Progress>,
    incoming: Receiver<MultiplayerEvent>,
    terminal: Receiver<MultiplayerError>,
    stop_flag: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    local: Option<Progress>,
    remote: Option<Progress>,
    closed: bool,
    connected: bool,
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
        let worker = thread::Builder::new()
            .name("bms-multiplayer".into())
            .spawn(move || {
                let result = run(endpoint, identity, options, &worker_stop, out_rx, in_tx);
                let reason = result.err().unwrap_or(MultiplayerError::Closed);
                let _ = terminal_tx.try_send(reason);
            })?;
        Ok(Self {
            outgoing,
            incoming,
            terminal,
            stop_flag,
            worker: Some(worker),
            local: None,
            remote: None,
            closed: false,
            connected: false,
        })
    }
    /// Gameplay-side admission only; performs no socket I/O and never waits for capacity.
    pub fn try_publish(&mut self, progress: Progress) -> Result<(), MultiplayerError> {
        if self.closed || self.stop_flag.load(Ordering::Acquire) {
            return Err(MultiplayerError::Closed);
        }
        validate_progress(self.local, progress)?;
        match self.outgoing.try_send(progress) {
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
    /// Drain a bounded batch; terminal notification follows all retained remote data.
    pub fn poll(&mut self) -> Vec<MultiplayerEvent> {
        // Worker may keep producing, so retain an explicit per-call bound.
        let mut events = Vec::new();
        for _ in 0..1024 {
            match self.incoming.try_recv() {
                Ok(event) => {
                    if event == MultiplayerEvent::Connected {
                        self.connected = true;
                    }
                    if let MultiplayerEvent::Progress(progress) = event {
                        self.remote = Some(progress);
                    }
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
                        if event == MultiplayerEvent::Connected {
                            self.connected = true;
                        }
                        if let MultiplayerEvent::Progress(progress) = event {
                            self.remote = Some(progress);
                        }
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
    frame(2, &payload)
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
    outgoing: Receiver<Progress>,
    incoming: SyncSender<MultiplayerEvent>,
) -> Result<(), MultiplayerError> {
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
    let mut tx_sequence = 0u64;
    let mut rx_sequence = 0u64;
    let mut previous = None;
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        if !connected && Instant::now() >= deadline {
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
            } else {
                if tag != 2 {
                    return Err(MultiplayerError::Protocol("unexpected message".into()));
                }
                let progress = parse_progress(&payload, rx_sequence, previous)?;
                rx_sequence = rx_sequence
                    .checked_add(1)
                    .ok_or_else(|| MultiplayerError::Protocol("sequence exhausted".into()))?;
                previous = Some(progress);
                incoming
                    .try_send(MultiplayerEvent::Progress(progress))
                    .map_err(queue_error)?;
            }
        }
        if connected && written == tx.len() {
            match outgoing.try_recv() {
                Ok(progress) => {
                    tx = progress_frame(tx_sequence, progress);
                    written = 0;
                    last_write = Instant::now();
                    tx_sequence = tx_sequence
                        .checked_add(1)
                        .ok_or_else(|| MultiplayerError::Protocol("sequence exhausted".into()))?;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
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
mod tests {
    use super::*;
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
        assert!(validate_progress(
            Some(value),
            Progress {
                song_ns: -3,
                ..value
            }
        )
        .is_err());
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
        decoder.bytes[8] = 2;
        assert!(decoder.take().is_err());
        assert!(validate_progress(
            None,
            Progress {
                hits: u64::MAX,
                misses: 1,
                combo: 0,
                max_combo: 0,
                song_ns: 0
            }
        )
        .is_err());
        assert!(validate_progress(
            None,
            Progress {
                combo: 3,
                ..progress()
            }
        )
        .is_err());
    }
}
