//! Native WebTransport client for the same reliable peer session used by QUIC.
#[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
use crate::multiplayer_start::StartRole;

pub use crate::webtransport_preparation::WebTransportOptions;
#[cfg(not(all(not(target_arch = "wasm32"), feature = "webtransport")))]
pub(crate) use crate::webtransport_preparation::unavailable;

#[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
pub use native::{WebTransportEndpoint, WebTransportStream};
#[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
pub(crate) use crate::webtransport_preparation::validate_url;
#[cfg(all(test, not(target_arch = "wasm32"), feature = "webtransport"))]
pub(crate) use native::client_config;

#[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
mod native {
    use super::{StartRole, WebTransportOptions, validate_url};
    use crate::multiplayer_quic::{client_tls, NativeCredentialReader, credential_error, wait};
    use crate::webtransport_preparation::prepare_webtransport;
    use std::{
        fmt,
        io::{self, Read, Write},
        sync::atomic::{AtomicBool, Ordering},
        time::{Duration, Instant},
    };
    use tokio::runtime::{Builder, Runtime};
    use wtransport::{
        config::IpBindConfig,
        endpoint::{endpoint_side::Client, ConnectOptions},
        ClientConfig, Connection, Endpoint, RecvStream, SendStream, VarInt,
    };

    const TICK: Duration = Duration::from_millis(5);
    fn invalid(message: &'static str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidInput, message)
    }
    fn network_error(error: impl fmt::Display) -> io::Error {
        io::Error::new(io::ErrorKind::ConnectionAborted, error.to_string())
    }
    fn timed_out() -> io::Error {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "WebTransport operation deadline elapsed",
        )
    }

    fn configured_client(ca: &[u8], bind: IpBindConfig) -> io::Result<ClientConfig> {
        if ca.is_empty() || ca.len() > 1024 * 1024 {
            return Err(invalid("WebTransport CA must contain 1..=1048576 bytes"));
        }
        if ca.windows(11).any(|window| window == b"-----BEGIN ") {
            crate::multiplayer_webtransport::pem_sections(ca, false)?;
        }
        let tls = client_tls(ca, wtransport::tls::WEBTRANSPORT_ALPN)?;
        let mut transport = quinn::TransportConfig::default();
        transport
            .max_concurrent_bidi_streams(quinn::VarInt::from_u32(0))
            .max_concurrent_uni_streams(quinn::VarInt::from_u32(8))
            .stream_receive_window(quinn::VarInt::from_u32(128 * 1024))
            .receive_window(quinn::VarInt::from_u32(512 * 1024))
            .send_window(512 * 1024)
            .datagram_receive_buffer_size(Some(16384))
            .datagram_send_buffer_size(16384)
            .max_idle_timeout(Some(
                Duration::from_secs(30).try_into().map_err(network_error)?,
            ))
            .keep_alive_interval(Some(Duration::from_secs(2)));
        Ok(ClientConfig::builder()
            .with_bind_config(bind)
            .with_custom_tls_and_transport(tls, transport)
            .build())
    }

    /// Actual socket-free configuration, with only explicit CA trust anchors.
    pub(crate) fn client_config(ca: &[u8]) -> io::Result<ClientConfig> {
        configured_client(ca, IpBindConfig::InAddrAnyDual)
    }

    /// Credential preparation only. The worker creates the runtime and socket.
    pub struct WebTransportEndpoint {
        options: WebTransportOptions,
        config: ClientConfig,
    }
    impl WebTransportEndpoint {
        pub fn prepare(options: &WebTransportOptions) -> io::Result<Self> {
            let prepared = prepare_webtransport(&mut NativeCredentialReader, options)
                .map_err(credential_error)?;
            let url = validate_url(&prepared.options.url)?;
            let ca = prepared.ca;
            let config = match url.host() {
                Some(url::Host::Ipv4(_)) => configured_client(&ca, IpBindConfig::InAddrAnyV4)?,
                Some(url::Host::Ipv6(_)) => configured_client(&ca, IpBindConfig::InAddrAnyV6)?,
                _ => client_config(&ca)?,
            };
            Ok(Self {
                options: options.clone(),
                config,
            })
        }
        pub fn role(&self) -> StartRole {
            self.options.role
        }
        /// Validate BKMR identity/roster before connecting the existing trusted
        /// endpoint. The caller drives admission, deadlines and stream disposal.
        /// The endpoint's bilateral start role does not grant room authority;
        /// only the actual first admitted participant may seal the room.
        pub fn connect_room(
            self,
            identity: &[u8],
            players: &[crate::local_players::PlayerId],
            stop: &AtomicBool,
            deadline: Instant,
        ) -> io::Result<crate::multiplayer_room_client::RoomClientIo<WebTransportStream>> {
            let session = crate::multiplayer_room_client::RoomClientSession::new(identity, players)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
            let stream = self.connect(stop, deadline)?;
            Ok(crate::multiplayer_room_client::RoomClientIo::new(
                session, stream,
            ))
        }
        /// Validate the actual room admission/start owner before connecting the
        /// existing trusted endpoint. The caller supplies elapsed observations,
        /// cancellation and deadlines while driving the returned stream owner.
        /// Room authority follows admission order, not the bilateral role option.
        /// A committed software schedule still needs actual output activation.
        pub fn connect_room_play(
            self,
            identity: &[u8],
            players: &[crate::local_players::PlayerId],
            policy: crate::multiplayer_start::StartPolicy,
            preroll_ns: i64,
            stop: &AtomicBool,
            deadline: Instant,
        ) -> io::Result<crate::multiplayer_room_io::RoomPlayIo<WebTransportStream>> {
            let session = crate::multiplayer_room_play::RoomPlayClient::new(
                identity, players, policy, preroll_ns,
            )
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
            let stream = self.connect(stop, deadline)?;
            Ok(crate::multiplayer_room_io::RoomPlayIo::new(session, stream))
        }
        pub fn connect(
            self,
            stop: &AtomicBool,
            deadline: Instant,
        ) -> io::Result<WebTransportStream> {
            if stop.load(Ordering::Acquire) {
                return Err(io::ErrorKind::Interrupted.into());
            }
            if Instant::now() >= deadline {
                return Err(timed_out());
            }
            let runtime = Builder::new_current_thread().enable_all().build()?;
            let endpoint = {
                let _entered = runtime.enter();
                Endpoint::client(self.config)
            };
            let endpoint = match endpoint {
                Ok(endpoint) => endpoint,
                Err(error) => {
                    runtime.shutdown_background();
                    return Err(error);
                }
            };
            let request = ConnectOptions::builder(self.options.url)
                .add_header("origin", self.options.origin)
                .build();
            let connected = wait(
                &runtime,
                async {
                    let connection = endpoint.connect(request).await.map_err(network_error)?;
                    // The second await emits the WebTransport stream header. It is
                    // retained inside the same pinned setup future across every tick.
                    let opened = connection.open_bi().await.map_err(network_error)?;
                    let (send, recv) = opened.await.map_err(network_error)?;
                    Ok::<_, io::Error>((connection, send, recv))
                },
                stop,
                deadline,
            )
            .and_then(|result| result);
            match connected {
                Ok((connection, send, recv)) => Ok(WebTransportStream {
                    send,
                    recv,
                    connection,
                    endpoint,
                    finished: false,
                    finish_error: None,
                    runtime: Some(runtime),
                }),
                Err(error) => {
                    endpoint.close(VarInt::from_u32(1), b"setup failed");
                    drop(endpoint);
                    // Tokio's resolver may have an unabortable blocking OS DNS
                    // task. Discard its result without blocking this worker's join.
                    runtime.shutdown_background();
                    Err(error)
                }
            }
        }
    }

    pub struct WebTransportStream {
        send: SendStream,
        recv: RecvStream,
        connection: Connection,
        endpoint: Endpoint<Client>,
        finished: bool,
        finish_error: Option<(io::ErrorKind, String)>,
        runtime: Option<Runtime>,
    }
    impl WebTransportStream {
        pub fn idle(&self, duration: Duration) {
            self.runtime
                .as_ref()
                .expect("live runtime")
                .block_on(async { tokio::time::sleep(duration).await });
        }
        pub fn finish(&mut self, limit: Duration) -> io::Result<()> {
            if self.finished {
                return match &self.finish_error {
                    Some((kind, text)) => Err(io::Error::new(*kind, text.clone())),
                    None => Ok(()),
                };
            }
            let deadline = Instant::now()
                .checked_add(limit)
                .ok_or_else(|| invalid("WebTransport finish timeout overflow"))?;
            self.finished = true;
            let runtime = self.runtime.as_ref().expect("live runtime");
            let drain = runtime.block_on(async {
                tokio::time::timeout(
                    deadline.saturating_duration_since(Instant::now()),
                    self.send.finish(),
                )
                .await
                .map_err(|_| timed_out())?
                .map_err(network_error)
            });
            self.connection
                .close(VarInt::from_u32(0), b"session finished");
            self.endpoint
                .close(VarInt::from_u32(0), b"session finished");
            let closed = runtime
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
        fn ensure_open(&self) -> io::Result<()> {
            if self.finished {
                Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "WebTransport stream finished",
                ))
            } else {
                Ok(())
            }
        }
    }
    impl Read for WebTransportStream {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if buffer.is_empty() {
                return Ok(0);
            }
            self.ensure_open()?;
            match self
                .runtime
                .as_ref()
                .expect("live runtime")
                .block_on(async { tokio::time::timeout(TICK, self.recv.read(buffer)).await })
            {
                Ok(Ok(count)) => Ok(count.unwrap_or(0)),
                Ok(Err(error)) => Err(network_error(error)),
                Err(_) => Err(io::ErrorKind::WouldBlock.into()),
            }
        }
    }
    impl Write for WebTransportStream {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if buffer.is_empty() {
                return Ok(0);
            }
            self.ensure_open()?;
            match self
                .runtime
                .as_ref()
                .expect("live runtime")
                .block_on(async { tokio::time::timeout(TICK, self.send.write(buffer)).await })
            {
                Ok(Ok(count)) => Ok(count),
                Ok(Err(error)) => Err(network_error(error)),
                Err(_) => Err(io::ErrorKind::WouldBlock.into()),
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            self.ensure_open()
        }
    }
    impl Drop for WebTransportStream {
        fn drop(&mut self) {
            if !self.finished {
                self.connection
                    .close(VarInt::from_u32(1), b"session abandoned");
                self.endpoint
                    .close(VarInt::from_u32(1), b"session abandoned");
            }
            if let Some(runtime) = self.runtime.take() {
                runtime.shutdown_background();
            }
        }
    }
}
