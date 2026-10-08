//! Deferred real QUIC integration: explicitly supplied TLS credentials and UDP sockets.
//!
//! These tests are compiled during source checks but require an explicit ignored-test
//! run. The certificate must be currently valid for the supplied server name and CA,
//! and must not authorize `beatkernel-quic-name-mismatch.invalid`.
#![cfg(not(target_arch = "wasm32"))]

use beatkernel::{
    input::CodecLimits,
    replay::{codec::ReplayCodecLimits, ReplayHeader, REPLAY_VERSION},
    time::ClockDomainId,
};
use beatkernel_bms_runtime::{
    multiplayer::{
        competition_identity, Multiplayer, MultiplayerError, MultiplayerEvent, MultiplayerOptions,
        Progress,
    },
    multiplayer_quic::QuicCredentials,
};
use std::fs;
use std::{
    env,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_secs(10);
const HOST_PREROLL: i64 = 10_000_000;
const JOIN_PREROLL: i64 = 25_000_000;
const WRONG_NAME: &str = "beatkernel-quic-name-mismatch.invalid";

struct Credentials {
    host: QuicCredentials,
    join: QuicCredentials,
}

impl Credentials {
    fn from_environment() -> Self {
        fn path(name: &str) -> PathBuf {
            let value = env::var_os(name)
                .unwrap_or_else(|| panic!("ignored QUIC integration requires {name}"));
            assert!(!value.is_empty(), "required {name} must not be empty");
            value.into()
        }
        let server_name = env::var("BEATKERNEL_TEST_QUIC_SERVER_NAME")
            .expect("ignored QUIC integration requires BEATKERNEL_TEST_QUIC_SERVER_NAME");
        assert!(
            !server_name.is_empty(),
            "required server name must not be empty"
        );
        let credentials = Self {
            host: QuicCredentials {
                cert: Some(path("BEATKERNEL_TEST_QUIC_CERT")),
                key: Some(path("BEATKERNEL_TEST_QUIC_KEY")),
                ..Default::default()
            },
            join: QuicCredentials {
                ca: Some(path("BEATKERNEL_TEST_QUIC_CA")),
                server_name: Some(server_name),
                ..Default::default()
            },
        };
        credentials.host.validate_for_role(true).unwrap();
        credentials.join.validate_for_role(false).unwrap();
        credentials
    }
}

fn identity(seed: u64) -> Vec<u8> {
    let limits = ReplayCodecLimits::new(65_536, 1, 4096, CodecLimits::new(64, 0).unwrap()).unwrap();
    competition_identity(
        &ReplayHeader {
            version: REPLAY_VERSION,
            chart_identity: b"quic-loopback-test-chart".to_vec(),
            rules_identity: b"quic-loopback-test-rules".to_vec(),
            options: b"quic-loopback-test-options".to_vec(),
            seed,
            normalized_clock: ClockDomainId(17),
        },
        "quic-loopback-test-runtime",
        limits,
    )
    .unwrap()
}

fn options(quic: QuicCredentials, preroll_ns: i64) -> MultiplayerOptions {
    MultiplayerOptions {
        quic,
        setup_timeout: Duration::from_secs(5),
        io_stall_timeout: Duration::from_secs(2),
        preroll_ns,
        ..Default::default()
    }
}

struct Peers {
    host: Multiplayer,
    join: Multiplayer,
    host_events: Vec<MultiplayerEvent>,
    join_events: Vec<MultiplayerEvent>,
}

impl Peers {
    fn connect(credentials: Credentials, host_seed: u64, join_seed: u64) -> Self {
        // The public host requires an explicit nonzero port. Release the probe
        // before the actual bind; an intervening port collision fails visibly.
        let address: SocketAddr = {
            let probe = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            probe.local_addr().unwrap()
        };
        let host = Multiplayer::host(
            address,
            identity(host_seed),
            options(credentials.host, HOST_PREROLL),
        )
        .expect("real QUIC host setup failed");
        // If join preparation panics, the already-created host's Drop still
        // cancels and joins its worker under the configured setup bound.
        let join = Multiplayer::join(
            address,
            identity(join_seed),
            options(credentials.join, JOIN_PREROLL),
        )
        .expect("real QUIC join setup failed");
        Self {
            host,
            join,
            host_events: Vec::new(),
            join_events: Vec::new(),
        }
    }

    fn poll_until(&mut self, deadline: Instant, label: &str, done: impl Fn(&Self) -> bool) {
        loop {
            self.host_events.extend(self.host.poll());
            self.join_events.extend(self.join.poll());
            if done(self) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {label}; host={:?}; join={:?}",
                self.host_events,
                self.join_events
            );
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn ready(&mut self) {
        self.host.try_ready().unwrap();
        self.join.try_ready().unwrap();
    }

    fn stop(&mut self) -> Result<(), MultiplayerError> {
        // Cancel both before either join, including when an assertion unwinds.
        self.host.request_stop();
        self.join.request_stop();
        let host = self.host.stop();
        let join = self.join.stop();
        host.and(join)
    }
}

impl Drop for Peers {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn terminal(events: &[MultiplayerEvent]) -> Option<&MultiplayerError> {
    events.iter().find_map(|event| match event {
        MultiplayerEvent::Disconnected(error) => Some(error),
        _ => None,
    })
}

fn progress_events(events: &[MultiplayerEvent]) -> Vec<Progress> {
    events
        .iter()
        .filter_map(|event| match event {
            MultiplayerEvent::Progress(progress) => Some(*progress),
            _ => None,
        })
        .collect()
}

fn final_events(events: &[MultiplayerEvent]) -> Vec<Progress> {
    events
        .iter()
        .filter_map(|event| match event {
            MultiplayerEvent::FinalProgress(progress) => Some(*progress),
            _ => None,
        })
        .collect()
}

fn acknowledged(events: &[MultiplayerEvent]) -> bool {
    events.contains(&MultiplayerEvent::FinalAcknowledged)
}

#[test]
#[ignore = "external test certificates and socket execution are deferred"]
fn compatible_peers_commit_start_exchange_exact_progress_and_acknowledge_both_finals() {
    let credentials = Credentials::from_environment();
    let deadline = Instant::now() + DEADLINE;
    let mut peers = Peers::connect(credentials, 41, 41);
    peers.ready();
    peers.poll_until(
        deadline,
        "bilateral readiness and committed start",
        |peers| {
            peers.host.is_ready()
                && peers.join.is_ready()
                && peers.host.start_schedule().is_some()
                && peers.join.start_schedule().is_some()
        },
    );
    for (owner, events, preroll) in [
        (&peers.host, &peers.host_events, HOST_PREROLL),
        (&peers.join, &peers.join_events, JOIN_PREROLL),
    ] {
        assert!(owner.is_connected());
        assert!(owner.clock_estimate().is_some());
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == MultiplayerEvent::Connected)
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == MultiplayerEvent::Ready)
                .count(),
            1
        );
        let schedule = owner.start_schedule().unwrap();
        assert!(schedule.target_ns > 0);
        assert_eq!(
            schedule.song_target_ns.checked_sub(schedule.target_ns),
            Some(preroll)
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == MultiplayerEvent::StartScheduled(schedule))
                .count(),
            1
        );
        assert!(
            terminal(events).is_none(),
            "peer disconnected before progress"
        );
    }

    let host_progress = Progress {
        song_ns: 1_000_000_000,
        hits: 7,
        misses: 1,
        combo: 3,
        max_combo: 5,
    };
    let join_progress = Progress {
        song_ns: 1_000_000_000,
        hits: 8,
        misses: 2,
        combo: 0,
        max_combo: 6,
    };
    peers.host.try_publish(host_progress).unwrap();
    peers.join.try_publish(join_progress).unwrap();
    peers.poll_until(
        deadline,
        "exact ordinary progress in both directions",
        |peers| {
            peers.host.remote_progress() == Some(join_progress)
                && peers.join.remote_progress() == Some(host_progress)
        },
    );
    assert_eq!(progress_events(&peers.host_events), vec![join_progress]);
    assert_eq!(progress_events(&peers.join_events), vec![host_progress]);

    let host_final = Progress {
        song_ns: 2_000_000_000,
        hits: 10,
        misses: 2,
        combo: 2,
        max_combo: 6,
    };
    let join_final = Progress {
        song_ns: 2_000_000_000,
        hits: 11,
        misses: 3,
        combo: 1,
        max_combo: 6,
    };
    peers.host.try_finish(host_final).unwrap();
    peers.join.try_finish(join_final).unwrap();
    peers.poll_until(
        deadline,
        "both exact final prefixes and application acknowledgements",
        |peers| {
            peers.host.remote_final_progress() == Some(join_final)
                && peers.join.remote_final_progress() == Some(host_final)
                && acknowledged(&peers.host_events)
                && acknowledged(&peers.join_events)
        },
    );
    assert_eq!(final_events(&peers.host_events), vec![join_final]);
    assert_eq!(final_events(&peers.join_events), vec![host_final]);
    assert_eq!(progress_events(&peers.host_events), vec![join_progress]);
    assert_eq!(progress_events(&peers.join_events), vec![host_progress]);
    for events in [&peers.host_events, &peers.join_events] {
        let ack = events
            .iter()
            .position(|event| *event == MultiplayerEvent::FinalAcknowledged)
            .unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == MultiplayerEvent::FinalAcknowledged)
                .count(),
            1
        );
        assert!(events[..ack]
            .iter()
            .all(|event| !matches!(event, MultiplayerEvent::Disconnected(_))));
    }
    // stop joins workers; its result is not evidence of transport-drain success.
    peers.stop().expect("QUIC worker join failed");
    assert!(!peers.host.is_connected() && !peers.join.is_connected());
    assert!(!peers.host.is_ready() && !peers.join.is_ready());
    assert_eq!(peers.host.remote_final_progress(), Some(join_final));
    assert_eq!(peers.join.remote_final_progress(), Some(host_final));
}

#[test]
#[ignore = "external test certificates and socket execution are deferred"]
fn certificate_name_mismatch_fails_tls_before_connected_or_ready() {
    let mut credentials = Credentials::from_environment();
    assert_ne!(credentials.join.server_name.as_deref(), Some(WRONG_NAME));
    credentials.join.server_name = Some(WRONG_NAME.into());
    let deadline = Instant::now() + DEADLINE;
    let mut peers = Peers::connect(credentials, 41, 41);
    peers.ready();
    peers.poll_until(
        deadline,
        "certificate-name rejection and worker termination",
        |peers| terminal(&peers.host_events).is_some() && terminal(&peers.join_events).is_some(),
    );
    match terminal(&peers.join_events).unwrap() {
        MultiplayerError::Io(message) => {
            let message = message.to_ascii_lowercase();
            assert!(
                message.contains("certificate") && message.contains("name"),
                "expected certificate-name diagnostic, got {message}"
            );
        }
        error => panic!("expected TLS certificate-name error, got {error:?}"),
    }
    for events in [&peers.host_events, &peers.join_events] {
        assert!(
            events
                .iter()
                .all(|event| matches!(event, MultiplayerEvent::Disconnected(_))),
            "TLS rejection exposed application session events: {events:?}"
        );
    }
    assert!(peers.host.start_schedule().is_none() && peers.join.start_schedule().is_none());
    assert!(!peers.host.is_ready() && !peers.join.is_ready());
    peers
        .stop()
        .expect("QUIC worker join failed after TLS rejection");
}

#[test]
#[ignore = "external unrelated development CA and real socket execution required"]
fn unrelated_root_ca_fails_tls_before_connected_ready_or_start_and_joins_both_workers() {
    let mut credentials = Credentials::from_environment();
    let wrong_ca = PathBuf::from(
        env::var_os("BEATKERNEL_TEST_QUIC_WRONG_CA")
            .expect("unrelated-root integration requires BEATKERNEL_TEST_QUIC_WRONG_CA"),
    );
    let wrong_bytes = fs::read(&wrong_ca)
        .expect("unrelated root must be an actual readable development certificate");
    assert!(!wrong_bytes.is_empty());
    assert_ne!(
        wrong_bytes,
        fs::read(credentials.join.ca.as_ref().unwrap()).unwrap(),
        "wrong-root test must not reuse the accepted CA bytes",
    );
    // Keep the correctly authorized server name and certificate. Change only
    // the genuine client's trusted root, separating CA rejection from name and
    // canonical application-identity rejection.
    credentials.join.ca = Some(wrong_ca);
    let mut peers = Peers::connect(credentials, 41, 41);
    peers.ready();
    peers.poll_until(
        Instant::now() + DEADLINE,
        "unrelated-root TLS rejection",
        |peers| terminal(&peers.host_events).is_some() && terminal(&peers.join_events).is_some(),
    );
    match terminal(&peers.join_events).unwrap() {
        MultiplayerError::Io(message) => {
            let message = message.to_ascii_lowercase();
            assert!(
                message.contains("certificate")
                    && (message.contains("unknownissuer")
                        || message.contains("unknown issuer")
                        || message.contains("unknown ca")),
                "expected certificate trust-root rejection, got {message}"
            );
        }
        error => panic!("expected unrelated-root TLS error, got {error:?}"),
    }
    for events in [&peers.host_events, &peers.join_events] {
        assert!(
            events
                .iter()
                .all(|event| matches!(event, MultiplayerEvent::Disconnected(_))),
            "untrusted root exposed application events: {events:?}"
        );
    }
    assert!(!peers.host.is_connected() && !peers.join.is_connected());
    assert!(!peers.host.is_ready() && !peers.join.is_ready());
    assert!(peers.host.start_schedule().is_none() && peers.join.start_schedule().is_none());
    assert!(peers.host.remote_progress().is_none() && peers.join.remote_progress().is_none());
    let stopped_at = Instant::now();
    peers
        .stop()
        .expect("both QUIC workers must join after unrelated-root refusal");
    assert!(stopped_at.elapsed() < Duration::from_secs(2));
}

#[test]
#[ignore = "external test certificates and socket execution are deferred"]
fn incompatible_canonical_identity_never_reaches_ready_or_scheduled_start() {
    let credentials = Credentials::from_environment();
    assert_ne!(identity(41), identity(42));
    let deadline = Instant::now() + DEADLINE;
    let mut peers = Peers::connect(credentials, 41, 42);
    // Request readiness before identity exchange to exercise its admission guard.
    peers.ready();
    peers.poll_until(deadline, "incompatible identity rejection", |peers| {
        terminal(&peers.host_events).is_some() && terminal(&peers.join_events).is_some()
    });
    assert!(
        terminal(&peers.host_events) == Some(&MultiplayerError::IncompatibleSetup)
            || terminal(&peers.join_events) == Some(&MultiplayerError::IncompatibleSetup),
        "no peer reported identity rejection: host={:?}; join={:?}",
        peers.host_events,
        peers.join_events
    );
    for events in [&peers.host_events, &peers.join_events] {
        assert!(
            events.iter().all(|event| matches!(
                event,
                MultiplayerEvent::Connected | MultiplayerEvent::Disconnected(_)
            )),
            "incompatible peer advanced application state: {events:?}"
        );
    }
    assert!(!peers.host.is_ready() && !peers.join.is_ready());
    assert!(peers.host.start_schedule().is_none() && peers.join.start_schedule().is_none());
    assert!(peers.host.remote_progress().is_none() && peers.join.remote_progress().is_none());
    peers
        .stop()
        .expect("QUIC worker join failed after incompatible setup");
}

#[test]
#[ignore = "external test certificates and socket execution are deferred"]
fn pending_accept_and_pending_connect_cancel_and_join_without_application_success() {
    let credentials = Credentials::from_environment();
    let host_address = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap();
    // An owned UDP sink holds the destination open without supplying any QUIC
    // response. Receipt proves the production join reached its real handshake;
    // the lone host remains in accept with no peer and no successful session.
    let sink = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    sink.set_nonblocking(true).unwrap();
    let host = Multiplayer::host(
        host_address,
        identity(41),
        options(credentials.host, HOST_PREROLL),
    )
    .expect("real pending QUIC host setup failed");
    let join = Multiplayer::join(
        sink.local_addr().unwrap(),
        identity(41),
        options(credentials.join, JOIN_PREROLL),
    )
    .expect("real pending QUIC join setup failed");
    let mut peers = Peers {
        host,
        join,
        host_events: Vec::new(),
        join_events: Vec::new(),
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut packet = [0u8; 2048];
    loop {
        peers.host_events.extend(peers.host.poll());
        peers.join_events.extend(peers.join.poll());
        assert!(peers.host_events.is_empty() && peers.join_events.is_empty());
        match sink.recv_from(&mut packet) {
            Ok((received, _)) => {
                assert!(
                    received >= 1200,
                    "QUIC Initial must reach the real UDP sink"
                );
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("real UDP sink failed: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "production join never sent its QUIC Initial"
        );
        thread::sleep(Duration::from_millis(2));
    }
    assert!(!peers.host.is_connected() && !peers.join.is_connected());
    assert!(!peers.host.is_ready() && !peers.join.is_ready());
    assert!(peers.host.start_schedule().is_none() && peers.join.start_schedule().is_none());
    let stopped_at = Instant::now();
    peers
        .stop()
        .expect("cancelled pending QUIC workers must join");
    assert!(stopped_at.elapsed() < Duration::from_secs(2));
    assert!(!peers.host.is_connected() && !peers.join.is_connected());
    assert!(!peers.host.is_ready() && !peers.join.is_ready());
    assert!(peers.host.remote_progress().is_none() && peers.join.remote_progress().is_none());
    let stopped_again = Instant::now();
    peers
        .stop()
        .expect("joined pending owners retain idempotent cleanup");
    assert!(stopped_again.elapsed() < Duration::from_secs(2));
}
