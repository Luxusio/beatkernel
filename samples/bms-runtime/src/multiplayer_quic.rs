//! Native reliable QUIC byte stream, owned by the existing multiplayer worker.
//! TLS authenticates the server, not the joining player or reported scores.
use crate::multiplayer_start::StartRole;
use std::{io, path::PathBuf};

/// Explicit trust material. Private key contents never enter this metadata.
#[derive(Clone, Debug, Default)]
pub struct QuicCredentials {
    pub cert: Option<PathBuf>,
    pub key: Option<PathBuf>,
    pub ca: Option<PathBuf>,
    pub server_name: Option<String>,
}

impl QuicCredentials {
    /// Validate a complete role before reading credentials or binding a socket.
    /// Paths retain native spelling and are bounded to 4096 encoded bytes.
    pub fn validate_for_role(&self, host: bool) -> io::Result<()> {
        for path in [&self.cert, &self.key, &self.ca].into_iter().flatten() {
            let bytes = path.as_os_str().as_encoded_bytes();
            if bytes.is_empty() || bytes.len() > 4096 || bytes.contains(&0) {
                return Err(invalid("invalid QUIC credential path"));
            }
        }
        if host {
            if self.cert.is_none()
                || self.key.is_none()
                || self.ca.is_some()
                || self.server_name.is_some()
            {
                return Err(invalid("QUIC host requires only certificate and key paths"));
            }
        } else {
            if self.cert.is_some() || self.key.is_some() || self.ca.is_none() {
                return Err(invalid("QUIC join requires only CA path and server name"));
            }
            validate_name(self.server_name.as_deref().unwrap_or_default())?;
        }
        Ok(())
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn validate_name(name: &str) -> io::Result<()> {
    if name.is_empty() || name.len() > 253 || !name.is_ascii() {
        return Err(invalid(
            "QUIC server name must be a bounded ASCII DNS name or IP",
        ));
    }
    if name.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }
    let dns = name.strip_suffix('.').unwrap_or(name);
    if dns.is_empty()
        || dns.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        || dns
            .rsplit('.')
            .next()
            .is_some_and(|label| label.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(invalid("invalid QUIC server name"));
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{QuicEndpoint, QuicStream};
#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) use native::{client_config, server_config};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{QuicCredentials, StartRole, invalid, validate_name};
    use quinn::{
        ClientConfig, Connection, Endpoint, EndpointConfig, RecvStream, SendStream, ServerConfig,
        TransportConfig, VarInt,
        crypto::rustls::{QuicClientConfig, QuicServerConfig},
        rustls::{
            self,
            pki_types::{CertificateDer, PrivateKeyDer, ServerName, pem::PemObject},
        },
    };
    use std::{
        fmt,
        fs::{self, File},
        future::Future,
        io::{self, Read, Write},
        net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
        path::Path,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };
    use tokio::runtime::{Builder, Runtime};

    const TICK: Duration = Duration::from_millis(5);
    const FILE_LIMIT: usize = 1024 * 1024;
    const WINDOW: u32 = 256 * 1024;
    const ALPN: &[u8] = b"beatkernel-multiplayer/6";

    fn data_error(message: &'static str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message)
    }
    fn network_error(error: impl fmt::Display) -> io::Error {
        io::Error::new(io::ErrorKind::ConnectionAborted, error.to_string())
    }
    fn timed_out() -> io::Error {
        io::Error::new(io::ErrorKind::TimedOut, "QUIC operation deadline elapsed")
    }
    fn cancelled() -> io::Error {
        io::Error::new(io::ErrorKind::Interrupted, "QUIC connection cancelled")
    }
    fn bounded_bytes(bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() || bytes.len() > FILE_LIMIT {
            return Err(data_error("QUIC credential must contain 1..=1048576 bytes"));
        }
        Ok(())
    }
    fn read_credential(path: &Path) -> io::Result<Vec<u8>> {
        // Check before open as well so ordinary directory/device/FIFO paths do
        // not enter the read path. Recheck the actual opened regular file.
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() || metadata.len() > FILE_LIMIT as u64 {
            return Err(invalid(
                "QUIC credential must be a regular file at most 1 MiB",
            ));
        }
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > FILE_LIMIT as u64 {
            return Err(invalid(
                "QUIC credential must be a regular file at most 1 MiB",
            ));
        }
        let mut bytes = Vec::new();
        file.take(FILE_LIMIT as u64 + 1).read_to_end(&mut bytes)?;
        bounded_bytes(&bytes)?;
        Ok(bytes)
    }
    fn is_pem(bytes: &[u8]) -> bool {
        bytes.windows(11).any(|window| window == b"-----BEGIN ")
    }
    fn pem_headers(bytes: &[u8], private_key: bool) -> io::Result<()> {
        // PemObject intentionally ignores unsupported sections. Reject those
        // explicitly here instead of silently accepting mixed credential files.
        for line in bytes.split(|byte| *byte == b'\n' || *byte == b'\r') {
            let line = line.trim_ascii();
            if !line.starts_with(b"-----BEGIN ") {
                continue;
            }
            let supported = if private_key {
                line == b"-----BEGIN PRIVATE KEY-----"
                    || line == b"-----BEGIN RSA PRIVATE KEY-----"
                    || line == b"-----BEGIN EC PRIVATE KEY-----"
            } else {
                line == b"-----BEGIN CERTIFICATE-----"
            };
            if !supported {
                return Err(data_error("unsupported QUIC credential PEM section"));
            }
        }
        Ok(())
    }
    fn certificates(bytes: &[u8]) -> io::Result<Vec<CertificateDer<'static>>> {
        bounded_bytes(bytes)?;
        if !is_pem(bytes) {
            return Ok(vec![CertificateDer::from(bytes.to_vec())]);
        }
        pem_headers(bytes, false)?;
        let certificates = CertificateDer::pem_slice_iter(bytes)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| data_error("invalid QUIC certificate PEM"))?;
        if certificates.is_empty() {
            return Err(data_error("QUIC certificate PEM contains no certificates"));
        }
        Ok(certificates)
    }
    fn private_key(bytes: &[u8]) -> io::Result<PrivateKeyDer<'static>> {
        bounded_bytes(bytes)?;
        if !is_pem(bytes) {
            return PrivateKeyDer::try_from(bytes.to_vec())
                .map_err(|_| data_error("invalid QUIC private key DER"));
        }
        pem_headers(bytes, true)?;
        let mut keys = PrivateKeyDer::pem_slice_iter(bytes);
        let key = keys
            .next()
            .ok_or_else(|| data_error("QUIC PEM contains no private key"))?
            .map_err(|_| data_error("invalid QUIC private key PEM"))?;
        if keys.next().is_some() {
            return Err(data_error("QUIC PEM must contain exactly one private key"));
        }
        Ok(key)
    }
    fn transport(host: bool) -> TransportConfig {
        let mut config = TransportConfig::default();
        config
            .max_concurrent_bidi_streams(VarInt::from_u32(u32::from(host)))
            .max_concurrent_uni_streams(VarInt::from_u32(0))
            .stream_receive_window(VarInt::from_u32(WINDOW))
            .receive_window(VarInt::from_u32(WINDOW))
            .send_window(u64::from(WINDOW))
            .datagram_receive_buffer_size(None)
            .datagram_send_buffer_size(0)
            .max_idle_timeout(Some(VarInt::from_u32(30_000).into()))
            .keep_alive_interval(Some(Duration::from_secs(5)));
        config
    }

    /// Actual socket-free host configuration path, also available to fixtures.
    pub(crate) fn server_config(cert_bytes: &[u8], key_bytes: &[u8]) -> io::Result<ServerConfig> {
        let chain = certificates(cert_bytes)?;
        let key = private_key(key_bytes)?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut tls = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|_| data_error("invalid QUIC TLS provider"))?
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .map_err(|_| data_error("invalid or mismatched QUIC certificate/private key"))?;
        tls.alpn_protocols = vec![ALPN.to_vec()];
        tls.max_early_data_size = 0;
        let crypto = QuicServerConfig::try_from(tls)
            .map_err(|_| data_error("invalid QUIC server TLS configuration"))?;
        let mut config = ServerConfig::with_crypto(Arc::new(crypto));
        config
            .transport_config(Arc::new(transport(true)))
            .max_incoming(1)
            .incoming_buffer_size(u64::from(WINDOW))
            .incoming_buffer_size_total(u64::from(WINDOW));
        Ok(config)
    }

    /// Actual socket-free join configuration; there is no insecure verifier.
    pub(crate) fn client_config(ca_bytes: &[u8], name: &str) -> io::Result<ClientConfig> {
        validate_name(name)?;
        ServerName::try_from(name).map_err(|_| invalid("invalid QUIC TLS server name"))?;
        let mut roots = rustls::RootCertStore::empty();
        for certificate in certificates(ca_bytes)? {
            roots
                .add(certificate)
                .map_err(|_| data_error("invalid QUIC CA certificate"))?;
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|_| data_error("invalid QUIC TLS provider"))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        tls.alpn_protocols = vec![ALPN.to_vec()];
        tls.enable_early_data = false;
        let crypto = QuicClientConfig::try_from(tls)
            .map_err(|_| data_error("invalid QUIC client TLS configuration"))?;
        let mut config = ClientConfig::new(Arc::new(crypto));
        config.transport_config(Arc::new(transport(false)));
        Ok(config)
    }

    enum Prepared {
        Host(ServerConfig),
        Join {
            config: ClientConfig,
            address: SocketAddr,
            name: String,
        },
    }

    /// Prepared credentials and explicit UDP binding; no runtime thread yet.
    pub struct QuicEndpoint {
        socket: UdpSocket,
        prepared: Prepared,
    }
    impl QuicEndpoint {
        pub fn host(address: SocketAddr, credentials: &QuicCredentials) -> io::Result<Self> {
            credentials.validate_for_role(true)?;
            if address.port() == 0 {
                return Err(invalid("QUIC host requires a nonzero port"));
            }
            let config = server_config(
                &read_credential(credentials.cert.as_deref().unwrap())?,
                &read_credential(credentials.key.as_deref().unwrap())?,
            )?;
            let socket = UdpSocket::bind(address)?;
            socket.set_nonblocking(true)?;
            Ok(Self {
                socket,
                prepared: Prepared::Host(config),
            })
        }
        pub fn join(address: SocketAddr, credentials: &QuicCredentials) -> io::Result<Self> {
            credentials.validate_for_role(false)?;
            if address.port() == 0 || address.ip().is_unspecified() {
                return Err(invalid("invalid QUIC join address"));
            }
            let name = credentials.server_name.as_deref().unwrap();
            let config =
                client_config(&read_credential(credentials.ca.as_deref().unwrap())?, name)?;
            let local = match address.ip() {
                IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            };
            let socket = UdpSocket::bind(SocketAddr::new(local, 0))?;
            socket.set_nonblocking(true)?;
            Ok(Self {
                socket,
                prepared: Prepared::Join {
                    config,
                    address,
                    name: name.to_owned(),
                },
            })
        }
        pub fn role(&self) -> StartRole {
            match &self.prepared {
                Prepared::Host(_) => StartRole::Host,
                Prepared::Join { .. } => StartRole::Join,
            }
        }
        /// Called only by the existing network worker. One runtime drives both
        /// Quinn tasks and timers; every setup wait shares the caller deadline.
        pub fn connect(self, stop: &AtomicBool, deadline: Instant) -> io::Result<QuicStream> {
            if stop.load(Ordering::Acquire) {
                return Err(cancelled());
            }
            if Instant::now() >= deadline {
                return Err(timed_out());
            }
            let runtime = Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()?;
            let (server, client) = match self.prepared {
                Prepared::Host(config) => (Some(config), None),
                Prepared::Join {
                    config,
                    address,
                    name,
                } => (None, Some((config, address, name))),
            };
            let mut endpoint = {
                let _entered = runtime.enter();
                Endpoint::new(
                    EndpointConfig::default(),
                    server,
                    self.socket,
                    Arc::new(quinn::TokioRuntime),
                )?
            };
            let connected: io::Result<_> = (|| {
                if let Some((config, address, name)) = client {
                    endpoint.set_default_client_config(config);
                    let connecting = endpoint.connect(address, &name).map_err(network_error)?;
                    let connection =
                        wait(&runtime, connecting, stop, deadline)?.map_err(network_error)?;
                    let (send, recv) = wait(&runtime, connection.open_bi(), stop, deadline)?
                        .map_err(network_error)?;
                    // The shared protocol writes identity first, making this
                    // stream visible to the host's accept_bi operation.
                    Ok((connection, send, recv))
                } else {
                    let incoming = wait(&runtime, endpoint.accept(), stop, deadline)?
                        .ok_or_else(|| network_error("QUIC endpoint closed"))?;
                    let connecting = incoming.accept().map_err(network_error)?;
                    let connection =
                        wait(&runtime, connecting, stop, deadline)?.map_err(network_error)?;
                    endpoint.set_server_config(None);
                    let (send, recv) = wait(&runtime, connection.accept_bi(), stop, deadline)?
                        .map_err(network_error)?;
                    Ok((connection, send, recv))
                }
            })();
            match connected {
                Ok((connection, send, recv)) => Ok(QuicStream {
                    send,
                    recv,
                    connection,
                    endpoint,
                    finished: false,
                    finish_error: None,
                    runtime,
                }),
                Err(error) => {
                    endpoint.close(VarInt::from_u32(1), b"setup failed");
                    Err(error)
                }
            }
        }
    }

    fn wait<F: Future>(
        runtime: &Runtime,
        future: F,
        stop: &AtomicBool,
        deadline: Instant,
    ) -> io::Result<F::Output> {
        // Timeout only borrows this pinned future. A tick does not cancel and
        // recreate a handshake or stream-opening operation.
        let mut future = std::pin::pin!(future);
        loop {
            if stop.load(Ordering::Acquire) {
                return Err(cancelled());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(timed_out());
            }
            match runtime.block_on(async {
                tokio::time::timeout(TICK.min(remaining), future.as_mut()).await
            }) {
                Ok(value) => {
                    if stop.load(Ordering::Acquire) {
                        return Err(cancelled());
                    }
                    return Ok(value);
                }
                Err(_) => continue,
            }
        }
    }

    /// One reliable ordered stream. Successful writes mean local QUIC admission.
    /// Call finish before dropping the owner to attempt bounded transport drain.
    pub struct QuicStream {
        send: SendStream,
        recv: RecvStream,
        connection: Connection,
        endpoint: Endpoint,
        finished: bool,
        finish_error: Option<(io::ErrorKind, String)>,
        // Keep drivers alive until the network handles have been dropped.
        runtime: Runtime,
    }
    impl QuicStream {
        pub fn idle(&self, duration: Duration) {
            self.runtime
                .block_on(async { tokio::time::sleep(duration).await });
        }
        /// Finish and drain within one deadline, then close the endpoint.
        /// stopped(None) proves transport receipt, not remote application reads;
        /// the shared protocol retains ownership of final-prefix acknowledgement.
        pub fn finish(&mut self, timeout: Duration) -> io::Result<()> {
            if self.finished {
                return match &self.finish_error {
                    Some((kind, message)) => Err(io::Error::new(*kind, message.clone())),
                    None => Ok(()),
                };
            }
            let deadline = Instant::now()
                .checked_add(timeout)
                .ok_or_else(|| invalid("QUIC finish timeout overflow"))?;
            self.finished = true;
            let drain = match self.send.finish() {
                Ok(()) => self.runtime.block_on(async {
                    match tokio::time::timeout(
                        deadline.saturating_duration_since(Instant::now()),
                        self.send.stopped(),
                    )
                    .await
                    {
                        Ok(Ok(None)) => Ok(()),
                        Ok(Ok(Some(_))) => Err(network_error("QUIC peer stopped the send stream")),
                        Ok(Err(error)) => Err(network_error(error)),
                        Err(_) => Err(timed_out()),
                    }
                }),
                Err(error) => Err(network_error(error)),
            };
            self.connection
                .close(VarInt::from_u32(0), b"session finished");
            self.endpoint
                .close(VarInt::from_u32(0), b"session finished");
            let closed = self
                .runtime
                .block_on(async {
                    tokio::time::timeout(
                        deadline.saturating_duration_since(Instant::now()),
                        self.endpoint.wait_idle(),
                    )
                    .await
                })
                .map_err(|_| timed_out());
            let result = drain.and(closed);
            if let Err(error) = &result {
                self.finish_error = Some((error.kind(), error.to_string()));
            }
            result
        }
    }
    impl Read for QuicStream {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if buffer.is_empty() {
                return Ok(0);
            }
            if self.finished {
                return Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "QUIC stream finished",
                ));
            }
            match self
                .runtime
                .block_on(async { tokio::time::timeout(TICK, self.recv.read(buffer)).await })
            {
                Ok(Ok(count)) => Ok(count.unwrap_or(0)),
                Ok(Err(error)) => Err(network_error(error)),
                Err(_) => Err(io::ErrorKind::WouldBlock.into()),
            }
        }
    }
    impl Write for QuicStream {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if buffer.is_empty() {
                return Ok(0);
            }
            if self.finished {
                return Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "QUIC stream finished",
                ));
            }
            match self
                .runtime
                .block_on(async { tokio::time::timeout(TICK, self.send.write(buffer)).await })
            {
                Ok(Ok(count)) => Ok(count),
                Ok(Err(error)) => Err(network_error(error)),
                Err(_) => Err(io::ErrorKind::WouldBlock.into()),
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            // Quinn writes already enter its send queue. This is not a peer ACK.
            if self.finished {
                return Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "QUIC stream finished",
                ));
            }
            Ok(())
        }
    }
    impl Drop for QuicStream {
        fn drop(&mut self) {
            if !self.finished {
                // Exceptional abandonment only. Normal worker exit calls finish
                // while this owner and its runtime are still alive.
                self.connection
                    .close(VarInt::from_u32(1), b"session abandoned");
                self.endpoint
                    .close(VarInt::from_u32(1), b"session abandoned");
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod unsupported {
    use super::{QuicCredentials, StartRole};
    use std::{
        io::{self, Read, Write},
        net::SocketAddr,
        sync::atomic::AtomicBool,
        time::{Duration, Instant},
    };
    fn unavailable() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "native QUIC sockets require a native host; browser WebTransport needs a separate adapter",
        )
    }
    pub struct QuicEndpoint {
        _private: (),
    }
    pub struct QuicStream {
        _private: (),
    }
    impl QuicEndpoint {
        pub fn host(_: SocketAddr, _: &QuicCredentials) -> io::Result<Self> {
            Err(unavailable())
        }
        pub fn join(_: SocketAddr, _: &QuicCredentials) -> io::Result<Self> {
            Err(unavailable())
        }
        pub fn role(&self) -> StartRole {
            unreachable!("unsupported endpoint cannot be constructed")
        }
        pub fn connect(self, _: &AtomicBool, _: Instant) -> io::Result<QuicStream> {
            Err(unavailable())
        }
    }
    impl QuicStream {
        pub fn idle(&self, _: Duration) {}
        pub fn finish(&mut self, _: Duration) -> io::Result<()> {
            Err(unavailable())
        }
    }
    impl Read for QuicStream {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(unavailable())
        }
    }
    impl Write for QuicStream {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(unavailable())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(unavailable())
        }
    }
}
#[cfg(target_arch = "wasm32")]
pub use unsupported::{QuicEndpoint, QuicStream};
