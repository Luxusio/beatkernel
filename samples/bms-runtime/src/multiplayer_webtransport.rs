//! Bounded HTTP/3 WebTransport stream relay; peers retain all BKMP session logic.
use crate::{
    multiplayer_protocol::{encode_frame, FrameDecoder},
    multiplayer_rooms::{JoinOutcome, ParticipantId, ParticipantTicket, RoomPolicy, RoomRegistry},
};
use quinn::rustls::{
    self,
    pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::{self, File},
    io::{self, Read},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{watch, OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
    time::{timeout, timeout_at, Instant},
};
use wtransport::{Connection, Endpoint, RecvStream, SendStream, ServerConfig, VarInt};

const FILE_LIMIT: usize = 1024 * 1024;
const DRAIN: Duration = Duration::from_secs(2);
pub const HELP: &str = "serve-multiplayer --bind IP:PORT --cert PATH --key PATH --origin ORIGIN\n\
  --origin ORIGIN              Repeat for up to 16 exact HTTPS or HTTP loopback origins\n\
  --allow-missing-origin       Explicitly admit native clients without Origin\n\
  --max-rooms N                1..4096 (default 64)\n\
  --max-key-bytes N            1..1024 (default 128)\n\
  --max-sessions N             2..8192, including setup (default 128)\n\
  --max-setups N               1..256 and <= sessions (default 16)\n\
  --waiting-ms N               1..86400000 (default 30000)\n\
  --setup-ms N                 1..60000 (default 10000)\n\
  --io-ms N                    1..120000 per complete frame read/write (default 10000)\n\
Clients connect to https://SERVER/rooms/ASCII_KEY. Ctrl+C closes all sessions.\n";

#[derive(Clone, Debug)]
pub struct ServerOptions {
    pub bind: SocketAddr,
    pub cert: PathBuf,
    pub key: PathBuf,
    pub origins: Vec<String>,
    pub allow_missing_origin: bool,
    pub max_rooms: usize,
    pub max_key_bytes: usize,
    pub max_sessions: usize,
    pub max_setups: usize,
    pub waiting_ttl: Duration,
    pub setup_timeout: Duration,
    pub io_timeout: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    Missing(&'static str),
    Duplicate(&'static str),
    UnknownOption,
    Invalid(&'static str),
}
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(flag) => write!(f, "missing {flag} or its value"),
            Self::Duplicate(flag) => write!(f, "duplicate {flag}"),
            Self::UnknownOption => f.write_str("unknown relay option"),
            Self::Invalid(flag) => write!(f, "invalid or out-of-range {flag}"),
        }
    }
}
impl std::error::Error for ConfigError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionError {
    InvalidPath,
    ForbiddenOrigin,
}

impl ServerOptions {
    pub fn parse(args: &[String]) -> Result<Self, ConfigError> {
        if args.len() > 64 || args.iter().any(|arg| arg.len() > 4096) {
            return Err(ConfigError::Invalid("arguments"));
        }
        let mut values = BTreeMap::new();
        let mut origins = Vec::new();
        let mut allow_missing_origin = false;
        let mut index = 0;
        while index < args.len() {
            let flag = match args[index].as_str() {
                "--bind" => "--bind",
                "--cert" => "--cert",
                "--key" => "--key",
                "--origin" => "--origin",
                "--max-rooms" => "--max-rooms",
                "--max-key-bytes" => "--max-key-bytes",
                "--max-sessions" => "--max-sessions",
                "--max-setups" => "--max-setups",
                "--waiting-ms" => "--waiting-ms",
                "--setup-ms" => "--setup-ms",
                "--io-ms" => "--io-ms",
                "--allow-missing-origin" => {
                    if allow_missing_origin {
                        return Err(ConfigError::Duplicate("--allow-missing-origin"));
                    }
                    allow_missing_origin = true;
                    index += 1;
                    continue;
                }
                _ => return Err(ConfigError::UnknownOption),
            };
            let value = args
                .get(index + 1)
                .filter(|value| !value.starts_with("--"))
                .ok_or(ConfigError::Missing(flag))?;
            if flag == "--origin" {
                if origins.contains(value) {
                    return Err(ConfigError::Duplicate(flag));
                }
                origins.push(value.clone());
            } else if values.insert(flag, value.as_str()).is_some() {
                return Err(ConfigError::Duplicate(flag));
            }
            index += 2;
        }
        let required = |flag| values.get(flag).copied().ok_or(ConfigError::Missing(flag));
        let number = |flag, default| -> Result<u64, ConfigError> {
            match values.get(flag) {
                Some(value)
                    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) =>
                {
                    value.parse().map_err(|_| ConfigError::Invalid(flag))
                }
                Some(_) => Err(ConfigError::Invalid(flag)),
                None => Ok(default),
            }
        };
        let options = Self {
            bind: required("--bind")?
                .parse()
                .map_err(|_| ConfigError::Invalid("--bind"))?,
            cert: PathBuf::from(required("--cert")?),
            key: PathBuf::from(required("--key")?),
            origins,
            allow_missing_origin,
            max_rooms: usize::try_from(number("--max-rooms", 64)?)
                .map_err(|_| ConfigError::Invalid("--max-rooms"))?,
            max_key_bytes: usize::try_from(number("--max-key-bytes", 128)?)
                .map_err(|_| ConfigError::Invalid("--max-key-bytes"))?,
            max_sessions: usize::try_from(number("--max-sessions", 128)?)
                .map_err(|_| ConfigError::Invalid("--max-sessions"))?,
            max_setups: usize::try_from(number("--max-setups", 16)?)
                .map_err(|_| ConfigError::Invalid("--max-setups"))?,
            waiting_ttl: Duration::from_millis(number("--waiting-ms", 30000)?),
            setup_timeout: Duration::from_millis(number("--setup-ms", 10000)?),
            io_timeout: Duration::from_millis(number("--io-ms", 10000)?),
        };
        options.validate()?;
        Ok(options)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.bind.port() == 0 {
            return Err(ConfigError::Invalid("--bind"));
        }
        for (flag, path) in [("--cert", &self.cert), ("--key", &self.key)] {
            let bytes = path.as_os_str().as_encoded_bytes();
            if bytes.is_empty() || bytes.len() > 4096 || bytes.contains(&0) {
                return Err(ConfigError::Invalid(flag));
            }
        }
        if self.origins.is_empty() {
            return Err(ConfigError::Missing("--origin"));
        }
        if self.origins.len() > 16 || self.origins.iter().any(|origin| !valid_origin(origin)) {
            return Err(ConfigError::Invalid("--origin"));
        }
        if self.origins.iter().collect::<BTreeSet<_>>().len() != self.origins.len() {
            return Err(ConfigError::Duplicate("--origin"));
        }
        for (flag, value, min, max) in [
            ("--max-rooms", self.max_rooms, 1, 4096),
            ("--max-key-bytes", self.max_key_bytes, 1, 1024),
            ("--max-sessions", self.max_sessions, 2, 8192),
            ("--max-setups", self.max_setups, 1, 256),
        ] {
            if !(min..=max).contains(&value) {
                return Err(ConfigError::Invalid(flag));
            }
        }
        if self.max_setups > self.max_sessions {
            return Err(ConfigError::Invalid("--max-setups"));
        }
        for (flag, value, max) in [
            ("--waiting-ms", self.waiting_ttl, 86400000),
            ("--setup-ms", self.setup_timeout, 60000),
            ("--io-ms", self.io_timeout, 120000),
        ] {
            if value < Duration::from_millis(1) || value > Duration::from_millis(max) {
                return Err(ConfigError::Invalid(flag));
            }
        }
        Ok(())
    }

    /// Validate the exact request before accepting a WebTransport session.
    pub fn request_room<'a>(
        &self,
        path: &'a str,
        origin: Option<&str>,
    ) -> Result<&'a str, AdmissionError> {
        if !match origin {
            Some(origin) => self.origins.iter().any(|allowed| allowed == origin),
            None => self.allow_missing_origin,
        } {
            return Err(AdmissionError::ForbiddenOrigin);
        }
        let key = path
            .strip_prefix("/rooms/")
            .ok_or(AdmissionError::InvalidPath)?;
        if key.is_empty()
            || key.len() > self.max_key_bytes
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(AdmissionError::InvalidPath);
        }
        Ok(key)
    }
}

pub(crate) fn valid_origin(origin: &str) -> bool {
    if origin.is_empty() || origin.len() > 4096 {
        return false;
    }
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    let loopback = match url.host() {
        Some(url::Host::Domain(name)) => name == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    (url.scheme() == "https" || (url.scheme() == "http" && loopback))
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.origin().ascii_serialization() == origin
}

fn invalid(error: impl fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
fn elapsed_timeout() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "relay operation timed out")
}
fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "relay stopped")
}

fn credential_bytes(path: &Path) -> io::Result<Vec<u8>> {
    let check = |metadata: fs::Metadata| -> io::Result<()> {
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > FILE_LIMIT as u64 {
            return Err(invalid(
                "credentials require a nonempty regular file of at most 1 MiB",
            ));
        }
        Ok(())
    };
    check(fs::metadata(path)?)?;
    let file = File::open(path)?;
    check(file.metadata()?)?;
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() > FILE_LIMIT {
        return Err(invalid("credential size changed"));
    }
    Ok(bytes)
}

pub(crate) fn pem_sections(bytes: &[u8], key: bool) -> io::Result<usize> {
    if bytes.is_empty() || bytes.len() > FILE_LIMIT {
        return Err(invalid("PEM must contain 1..=1048576 bytes"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("invalid PEM text"))?;
    let mut active = None;
    let mut sections = 0;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(label) = line
            .strip_prefix("-----BEGIN ")
            .and_then(|line| line.strip_suffix("-----"))
        {
            if active.is_some()
                || if key {
                    !matches!(label, "PRIVATE KEY" | "RSA PRIVATE KEY" | "EC PRIVATE KEY")
                } else {
                    label != "CERTIFICATE"
                }
            {
                return Err(invalid("unsupported or nested PEM section"));
            }
            active = Some(label);
        } else if let Some(label) = line
            .strip_prefix("-----END ")
            .and_then(|line| line.strip_suffix("-----"))
        {
            if active != Some(label) {
                return Err(invalid("unmatched PEM section"));
            }
            active = None;
            sections += 1;
        } else if active.is_none()
            || !line
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
        {
            return Err(invalid("unexpected PEM content"));
        }
    }
    if active.is_some() || sections == 0 || (key && sections != 1) {
        return Err(invalid("incomplete PEM credential"));
    }
    Ok(sections)
}

pub(crate) fn tls_from_pem(cert: &[u8], key: &[u8]) -> io::Result<rustls::ServerConfig> {
    let count = pem_sections(cert, false)?;
    pem_sections(key, true)?;
    let chain = CertificateDer::pem_slice_iter(cert)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid("invalid certificate PEM"))?;
    if chain.len() != count {
        return Err(invalid("incomplete certificate chain"));
    }
    let key = PrivateKeyDer::from_pem_slice(key).map_err(|_| invalid("invalid private key PEM"))?;
    let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(invalid)?
    .with_no_client_auth()
    .with_single_cert(chain, key)
    .map_err(|_| invalid("invalid certificate chain or private key"))?;
    tls.alpn_protocols = vec![wtransport::tls::WEBTRANSPORT_ALPN.to_vec()];
    tls.max_early_data_size = 0;
    Ok(tls)
}

/// Complete validated frames only; partial or malformed trailing data is never forwarded.
async fn forward<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    mut read: R,
    mut write: W,
    limit: Duration,
) -> io::Result<W> {
    let mut decoder = FrameDecoder::new();
    let mut scratch = [0u8; 4096];
    loop {
        let frame = timeout(limit, async {
            loop {
                let needed = decoder.needed().map_err(invalid)?;
                if needed == 0 {
                    return decoder.take().map_err(invalid);
                }
                let count = read.read(&mut scratch[..needed.min(4096)]).await?;
                if count == 0 {
                    if decoder.bytes.is_empty() {
                        return Ok(None);
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "truncated multiplayer frame",
                    ));
                }
                decoder.push(&scratch[..count]).map_err(invalid)?;
            }
        })
        .await
        .map_err(|_| elapsed_timeout())??;
        let Some((tag, payload)) = frame else {
            return Ok(write);
        };
        let bytes = encode_frame(tag, &payload).map_err(invalid)?;
        timeout(limit, write.write_all(&bytes))
            .await
            .map_err(|_| elapsed_timeout())??;
    }
}

pub(crate) struct RelayWriters<A, B> {
    pub(crate) first: A,
    pub(crate) second: B,
    pub(crate) deadline: Instant,
}

async fn wait_stop(stop: &mut watch::Receiver<bool>) {
    loop {
        if *stop.borrow() {
            return;
        }
        if stop.changed().await.is_err() {
            return;
        }
    }
}

/// Finite framed backpressure, with one shared two-second deadline after first EOF.
/// Native callers retain the returned writers to await actual transport delivery.
pub(crate) async fn relay_pair<AR, AW, BR, BW>(
    a_read: AR,
    a_write: AW,
    b_read: BR,
    b_write: BW,
    io_timeout: Duration,
    mut stop: watch::Receiver<bool>,
) -> io::Result<RelayWriters<AW, BW>>
where
    AR: AsyncRead + Unpin,
    AW: AsyncWrite + Unpin,
    BR: AsyncRead + Unpin,
    BW: AsyncWrite + Unpin,
{
    if io_timeout.is_zero() || io_timeout > Duration::from_secs(120) {
        return Err(invalid("invalid relay I/O timeout"));
    }
    let left = forward(a_read, b_write, io_timeout);
    let right = forward(b_read, a_write, io_timeout);
    tokio::pin!(left, right);
    tokio::select! {
        biased;
        _ = wait_stop(&mut stop) => Err(cancelled()),
        result = async {
            tokio::select! {
                result = &mut left => {
                    let mut second = result?;
                    let deadline = Instant::now() + DRAIN;
                    let first = timeout_at(deadline, async {
                        second.shutdown().await?;
                        let mut first = right.await?;
                        first.shutdown().await?;
                        Ok::<_, io::Error>(first)
                    }).await.map_err(|_| elapsed_timeout())??;
                    Ok(RelayWriters { first, second, deadline })
                }
                result = &mut right => {
                    let mut first = result?;
                    let deadline = Instant::now() + DRAIN;
                    let second = timeout_at(deadline, async {
                        first.shutdown().await?;
                        let mut second = left.await?;
                        second.shutdown().await?;
                        Ok::<_, io::Error>(second)
                    }).await.map_err(|_| elapsed_timeout())??;
                    Ok(RelayWriters { first, second, deadline })
                }
            }
        } => result,
    }
}

struct Resource {
    connection: Connection,
    stream: Option<(SendStream, RecvStream)>,
    _permit: OwnedSemaphorePermit,
}
impl Drop for Resource {
    fn drop(&mut self) {
        self.connection
            .close(VarInt::from_u32(0), b"relay resource released");
    }
}
struct Prepared {
    key: String,
    resource: Resource,
}

async fn prepare(
    incoming: wtransport::endpoint::IncomingSession,
    permit: OwnedSemaphorePermit,
    options: Arc<ServerOptions>,
) -> io::Result<Prepared> {
    timeout(options.setup_timeout, async {
        let request = incoming.await.map_err(invalid)?;
        let key = match options.request_room(request.path(), request.origin()) {
            Ok(key) => key.to_owned(),
            Err(AdmissionError::InvalidPath) => {
                request.not_found().await;
                return Err(invalid("invalid room path"));
            }
            Err(AdmissionError::ForbiddenOrigin) => {
                request.forbidden().await;
                return Err(invalid("forbidden Origin"));
            }
        };
        let connection = request.accept().await.map_err(invalid)?;
        let mut resource = Resource {
            connection,
            stream: None,
            _permit: permit,
        };
        resource.stream = Some(resource.connection.accept_bi().await.map_err(invalid)?);
        Ok(Prepared { key, resource })
    })
    .await
    .map_err(|_| elapsed_timeout())?
}

fn release_resources(
    resources: &mut BTreeMap<ParticipantId, Resource>,
    tickets: Vec<ParticipantTicket>,
) {
    for ticket in tickets {
        resources.remove(&ticket.id);
    }
}

fn now(origin: Instant) -> io::Result<i64> {
    i64::try_from(origin.elapsed().as_nanos()).map_err(|_| invalid("relay clock overflow"))
}

fn configuration(options: &ServerOptions) -> io::Result<ServerConfig> {
    options
        .validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let tls = tls_from_pem(
        &credential_bytes(&options.cert)?,
        &credential_bytes(&options.key)?,
    )?;
    let mut transport = quinn::TransportConfig::default();
    transport
        .max_concurrent_bidi_streams(quinn::VarInt::from_u32(2)) // CONNECT + one application stream
        .max_concurrent_uni_streams(quinn::VarInt::from_u32(8)) // HTTP/3 control and QPACK
        .stream_receive_window(quinn::VarInt::from_u32(128 * 1024))
        .receive_window(quinn::VarInt::from_u32(512 * 1024))
        .send_window(512 * 1024)
        // HTTP/3 WebTransport requires QUIC DATAGRAM capability. These bounded
        // buffers advertise it; application datagrams are never relayed.
        .datagram_receive_buffer_size(Some(16384))
        .datagram_send_buffer_size(16384)
        .max_idle_timeout(Some(Duration::from_secs(120).try_into().map_err(invalid)?))
        .keep_alive_interval(Some(Duration::from_secs(2)));
    let mut config = ServerConfig::builder()
        .with_bind_address(options.bind)
        .with_custom_tls_and_transport(tls, transport)
        .build();
    config
        .quic_config_mut()
        .max_incoming(options.max_setups)
        .incoming_buffer_size(32768)
        .incoming_buffer_size_total(options.max_setups as u64 * 32768);
    Ok(config)
}

/// Run the actual endpoint on one current-thread Tokio owner until Ctrl+C.
pub fn run(options: ServerOptions) -> io::Result<()> {
    let config = configuration(&options)?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(serve(options, config))
}

async fn serve(options: ServerOptions, config: ServerConfig) -> io::Result<()> {
    let endpoint = Endpoint::server(config)?;
    let policy = RoomPolicy::new(
        options.max_rooms,
        options.max_key_bytes,
        i64::try_from(options.waiting_ttl.as_nanos()).map_err(invalid)?,
    )
    .map_err(invalid)?;
    let mut registry = RoomRegistry::new(policy);
    let options = Arc::new(options);
    let permits = Arc::new(Semaphore::new(options.max_sessions));
    let mut resources = BTreeMap::<ParticipantId, Resource>::new();
    let mut setups = JoinSet::<io::Result<Prepared>>::new();
    let mut relays = JoinSet::<(ParticipantId, io::Result<()>)>::new();
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
                finished = relays.join_next(), if !relays.is_empty() => {
                    let (id, _result) = finished.expect("nonempty relay tasks").map_err(invalid)?;
                    release_resources(&mut resources, registry.release(id, now(origin)?).map_err(invalid)?);
                }
                finished = setups.join_next(), if !setups.is_empty() => {
                    let result = finished.expect("nonempty setup tasks").map_err(invalid)?;
                    let Ok(prepared) = result else { continue; };
                    let time = now(origin)?;
                    release_resources(&mut resources, registry.expire(time).map_err(invalid)?);
                    match registry.join(&prepared.key, time) {
                        Ok(JoinOutcome::Waiting { ticket, .. }) => { resources.insert(ticket.id, prepared.resource); }
                        Ok(JoinOutcome::Paired { waiting, joined }) => {
                            resources.insert(joined.id, prepared.resource);
                            let (a_write, a_read) = resources.get_mut(&waiting.id).expect("waiting lease owns resource").stream.take().expect("waiting stream");
                            let (b_write, b_read) = resources.get_mut(&joined.id).expect("joined lease owns resource").stream.take().expect("joined stream");
                            let io_timeout = options.io_timeout;
                            let stopped = stopped.clone();
                            relays.spawn(async move {
                                let result = async {
                                    let mut writers = relay_pair(a_read, a_write, b_read, b_write, io_timeout, stopped).await?;
                                    timeout_at(writers.deadline, async {
                                        tokio::try_join!(writers.first.finish(), writers.second.finish()).map_err(invalid)?;
                                        Ok::<_, io::Error>(())
                                    }).await.map_err(|_| elapsed_timeout())?
                                }.await;
                                (waiting.id, result)
                            });
                        }
                        Err(_) => { drop(prepared); } // Resource guard closes rejected admission.
                    }
                }
                _ = expiry.tick() => {
                    let time = now(origin)?;
                    release_resources(&mut resources, registry.expire(time).map_err(invalid)?);
                    let closed: Vec<_> = resources.iter().filter(|(_, resource)| resource.connection.quic_connection().close_reason().is_some()).map(|(id, _)| *id).collect();
                    for id in closed { release_resources(&mut resources, registry.release(id, time).map_err(invalid)?); }
                }
                incoming = endpoint.accept() => {
                    if setups.len() >= options.max_setups || relays.len() >= options.max_sessions {
                        incoming.refuse();
                    } else if let Ok(permit) = permits.clone().try_acquire_owned() {
                        setups.spawn(prepare(incoming, permit, options.clone()));
                    } else { incoming.refuse(); }
                }
            }
        }
    }.await;
    let _ = stop.send(true);
    endpoint.close(VarInt::from_u32(0), b"relay shutting down");
    resources.clear();
    setups.abort_all();
    relays.abort_all();
    while setups.join_next().await.is_some() {}
    while relays.join_next().await.is_some() {}
    let _ = timeout(DRAIN, endpoint.wait_idle()).await;
    result
}
