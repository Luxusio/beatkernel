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

// These cases cross the actual selected native cohort admission/metadata bridge.
// Progress is a self-reported network prefix; no native completed-play flag is set.
mod selected_policy_peers {
    use super::*;
    use beatkernel::{
        audio::{AudioFormat, PcmLimits, SampleBank},
        input::DeviceId,
        judge::{JudgeGrade, JudgeWindow},
        time::{Duration as SongDuration, Timestamp},
    };
    use beatkernel_bms::{BmsGaugeKind, BmsJudgment};
    use beatkernel_bms_runtime::{
        competition_live::{CompetitionOptions, NetworkRole},
        competition_presentation::{NetworkSnapshot, NetworkStatus},
        gameplay_competition::GroupCompetitionPort,
        local_players::PlayerId,
        multiplayer_group::MemberProgress,
        native_cohort_setup::{
            prepare_audio_cohort_with_policy, CohortPreparation, PreparedCohort,
        },
        native_group_competition::NativeGroupCompetition,
        native_start::NativeStartAgreement,
        play_policy::{ClassifiedWindow, ResolvedPlayPolicy},
        replay_playback::decode_section_setup,
        PreparedBms,
    };
    use std::{collections::BTreeMap, sync::Barrier};

    #[derive(Clone, Copy)]
    struct Selected {
        gauge: BmsGaugeKind,
        class: BmsJudgment,
        offset: i64,
        start: i64,
        end: i64,
        seed: u64,
    }
    impl Default for Selected {
        fn default() -> Self {
            Self {
                gauge: BmsGaugeKind::Hard,
                class: BmsJudgment::Great,
                offset: -3,
                start: 1,
                end: 5_000_000_000,
                seed: 71,
            }
        }
    }
    fn cohort(
        role: NetworkRole,
        credentials: QuicCredentials,
        first: u32,
        selected: Selected,
        preroll: i64,
    ) -> PreparedCohort {
        let source = beatkernel_bms::parse_seeded(
            "#BPM 60\n#TOTAL 320\n#WAV01 note.wav\n#00111:0101\n",
            Default::default(),
            selected.seed,
        )
        .unwrap();
        let policy = ResolvedPlayPolicy::bms(
            &source,
            selected.gauge,
            &[ClassifiedWindow {
                judgment: selected.class,
                window: JudgeWindow {
                    grade: JudgeGrade(7),
                    early: SongDuration::from_nanos(7),
                    late: SongDuration::from_nanos(9),
                },
            }],
            selected.offset,
        )
        .unwrap();
        let prepared = PreparedBms {
            compiled: source.compile().unwrap(),
            source,
            bank: SampleBank::new(
                AudioFormat::new(1000, 1).unwrap(),
                PcmLimits::new(64, 256, 4).unwrap(),
            )
            .unwrap(),
            sounds: vec![],
            bgm_commands: vec![],
        };
        let bindings = BTreeMap::from([(0x11, 7)]);
        let assignments = [
            (PlayerId(first), DeviceId(31)),
            (PlayerId(first + 1), DeviceId(u64::MAX)),
        ];
        let options = CompetitionOptions {
            network: Some(role),
            quic: credentials,
            setup_timeout: Duration::from_secs(5),
            ..Default::default()
        };
        let cohort = prepare_audio_cohort_with_policy(
            &prepared,
            &assignments,
            &options,
            &CohortPreparation {
                host: ClockDomainId(99),
                output: ClockDomainId(2),
                early: 7,
                late: 9,
                offset: selected.offset,
                preroll,
                start: Timestamp::from_nanos(selected.start),
                end: Some(Timestamp::from_nanos(selected.end)),
                chart_seed: selected.seed,
                bindings: &bindings,
                record_replay: None,
                replay_max_bytes: 0,
                replay_max_records: 0,
            },
            ClockDomainId(17),
            &policy,
        )
        .unwrap();
        let network = cohort.network.as_ref().unwrap();
        let port: &dyn GroupCompetitionPort = network;
        assert!(!port.policy_agnostic());
        assert!(port.expected_policy_header(PlayerId(0)).is_none());
        for (member, state) in cohort.configs.iter().zip(&cohort.states) {
            assert!(state.capture.is_none());
            assert_eq!(state.gauge.profile(), policy.gauge());
            assert!(member.judge.effective_song_time().is_none());
            let header = port.expected_policy_header(member.player).unwrap();
            assert!(std::ptr::eq(
                header,
                network.native_policy_header(member.player).unwrap()
            ));
            let expected = beatkernel_bms_runtime::native_judge::prepare_policy_header(
                &prepared.source,
                &member.judge,
                &policy,
                ClockDomainId(17),
                Timestamp::from_nanos(selected.start),
                selected.seed,
                Some(Timestamp::from_nanos(selected.end)),
            )
            .unwrap();
            assert_eq!(header, &expected);
            let setup = decode_section_setup(&header.options).unwrap();
            assert_eq!(setup.judgments.as_ref(), policy.judgments());
            assert_eq!(setup.gauge, *policy.gauge());
            assert_eq!(setup.start, Timestamp::from_nanos(selected.start));
            assert_eq!(setup.end, Some(Timestamp::from_nanos(selected.end)));
            assert_eq!(setup.chart_seed, selected.seed);
        }
        cohort
    }

    struct SelectedPeers {
        host: NativeGroupCompetition,
        join: NativeGroupCompetition,
        // Retain the real prepared judges/gauges to prove remote progress never judges them.
        cohorts: [PreparedCohort; 2],
        finished: bool,
    }
    impl SelectedPeers {
        fn connect(selected: Selected) -> Self {
            let credentials = Credentials::from_environment();
            let address = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
                .unwrap()
                .local_addr()
                .unwrap();
            let mut host = cohort(
                NetworkRole::Host(address),
                credentials.host,
                11,
                Selected::default(),
                HOST_PREROLL,
            );
            let mut join = cohort(
                NetworkRole::Join(address),
                credentials.join,
                21,
                selected,
                JOIN_PREROLL,
            );
            Self {
                host: host.network.take().unwrap(),
                join: join.network.take().unwrap(),
                cohorts: [host, join],
                finished: false,
            }
        }
        fn commit(&mut self) -> Result<(), String> {
            // Host Ready is enqueued before the service enters Join's real gate.
            // Each existing production QUIC worker independently drives the handshake.
            let mut join_committed = false;
            let mut join_checked = false;
            let mut join_error = None;
            let host_result = self.host.await_commit(&mut || {
                if !join_checked {
                    join_checked = true;
                    match self.join.await_commit(&mut || Ok(true)) {
                        Ok(true) => join_committed = true,
                        Ok(false) => return Ok(false),
                        Err(error) => join_error = Some(error.to_string()),
                    }
                }
                Ok(true)
            });
            match host_result {
                Err(error) => Err(format!("host={error}; join={join_error:?}")),
                Ok(true) if join_committed => Ok(()),
                Ok(_) => Err(join_error.unwrap_or_else(|| "actual startup cancelled".into())),
            }
        }
        fn finish(
            &mut self,
            host: &[MemberProgress],
            join: &[MemberProgress],
        ) -> [Result<(), String>; 2] {
            let barrier = Barrier::new(2);
            let result = thread::scope(|scope| {
                let joining = &mut self.join;
                let barrier = &barrier;
                let worker = scope.spawn(move || {
                    barrier.wait();
                    joining.finish(join).map_err(|error| error.to_string())
                });
                barrier.wait();
                let host = self.host.finish(host).map_err(|error| error.to_string());
                [
                    host,
                    worker.join().expect("selected join finalization panicked"),
                ]
            });
            self.finished = true;
            result
        }
        fn pristine(&self) {
            for cohort in &self.cohorts {
                for (member, state) in cohort.configs.iter().zip(&cohort.states) {
                    assert!(member.judge.effective_song_time().is_none());
                    assert_eq!(
                        state.gauge.snapshot(),
                        beatkernel_bms_runtime::gauge::BmsGauge::new(
                            state.gauge.profile().try_copy().unwrap()
                        )
                        .snapshot()
                    );
                    assert!(state.capture.is_none());
                }
            }
        }
    }
    impl Drop for SelectedPeers {
        fn drop(&mut self) {
            if !self.finished {
                // Refused setup has no local prefix. Even a failing-progress unwind
                // must attempt both existing cleanup owners; no completion is asserted.
                let _ = self.finish(&[], &[]);
            }
        }
    }
    fn members(first: u32, tick: u64) -> Vec<MemberProgress> {
        (0..2)
            .map(|slot| MemberProgress {
                player: PlayerId(first + slot),
                progress: Progress {
                    song_ns: tick as i64 * 1_000_000_000,
                    hits: tick * 4 + u64::from(slot),
                    misses: tick,
                    combo: tick,
                    max_combo: tick * 2,
                },
            })
            .collect()
    }
    fn exact(rows: &[(PlayerId, NetworkSnapshot)], local: u32, remote: &[MemberProgress]) -> bool {
        rows.len() == 2
            && rows
                .iter()
                .zip(remote)
                .enumerate()
                .all(|(slot, ((player, snapshot), member))| {
                    *player == PlayerId(local + slot as u32)
                        && snapshot.progress == Some(member.progress)
                })
    }

    #[test]
    #[ignore = "requires ephemeral valid QUIC PKI and real UDP peers; selected native policy bridge"]
    fn actual_selected_cohort_peers_commit_exchange_ordered_prefixes_ack_finals_and_join() {
        let mut peers = SelectedPeers::connect(Selected::default());
        peers
            .commit()
            .expect("matching admitted selected cohorts must commit");
        for (owner, preroll) in [(&peers.host, HOST_PREROLL), (&peers.join, JOIN_PREROLL)] {
            let schedule = owner.committed_schedule().unwrap();
            assert!(schedule.target_ns > 0);
            assert_eq!(schedule.song_target_ns - schedule.target_ns, preroll);
            assert!(!owner.is_failed());
            assert!(
                !owner.native_completed(),
                "transport receipt is not native completion"
            );
        }
        for tick in 1..=2 {
            let host = members(11, tick);
            let join = members(21, tick);
            let deadline = Instant::now() + DEADLINE;
            loop {
                peers.host.observe(&host).unwrap();
                peers.join.observe(&join).unwrap();
                assert!(!peers.host.is_failed() && !peers.join.is_failed());
                if exact(&peers.host.archive_snapshots().unwrap(), 11, &join)
                    && exact(&peers.join.archive_snapshots().unwrap(), 21, &host)
                {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "selected peer prefix {tick} did not cross actual QUIC"
                );
                thread::sleep(Duration::from_millis(2));
            }
            peers.pristine();
        }
        let host_final = members(11, 3);
        let join_final = members(21, 3);
        let began = Instant::now();
        for result in peers.finish(&host_final, &join_final) {
            result.expect("selected final delivery/ACK and actual worker joins must succeed");
        }
        assert!(began.elapsed() < DEADLINE);
        let host = peers.host.archive_snapshots().unwrap();
        let join = peers.join.archive_snapshots().unwrap();
        assert!(exact(&host, 11, &join_final));
        assert!(exact(&join, 21, &host_final));
        assert!(host
            .iter()
            .chain(&join)
            .all(|(_, snapshot)| snapshot.status == NetworkStatus::Stopped));
        peers.pristine();
    }

    #[test]
    #[ignore = "requires ephemeral valid QUIC PKI and real UDP peers; admitted policy mismatches"]
    fn actual_selected_cohort_peers_refuse_class_gauge_profile_section_and_seed_before_start() {
        let selected = Selected::default();
        // Great and PGreat have the same Hard gauge effects and judge windows;
        // their genuine class mapping alone must still refuse the peer.
        let mut cases = vec![
            Selected {
                class: BmsJudgment::PGreat,
                ..selected
            },
            Selected {
                gauge: BmsGaugeKind::Hazard,
                ..selected
            },
            Selected {
                offset: selected.offset + 1,
                ..selected
            },
            Selected {
                start: selected.start + 1,
                ..selected
            },
            Selected {
                end: selected.end - 1,
                ..selected
            },
            Selected {
                seed: selected.seed + 1,
                ..selected
            },
        ];
        for (index, foreign) in cases.drain(..).enumerate() {
            let mut peers = SelectedPeers::connect(foreign);
            if index == 0 {
                assert_eq!(
                    peers.cohorts[0].configs[0].judge.profile(),
                    peers.cohorts[1].configs[0].judge.profile()
                );
                assert_eq!(
                    peers.cohorts[0].states[0].gauge.profile(),
                    peers.cohorts[1].states[0].gauge.profile()
                );
                assert_ne!(
                    peers
                        .host
                        .native_policy_header(PlayerId(11))
                        .unwrap()
                        .options,
                    peers
                        .join
                        .native_policy_header(PlayerId(21))
                        .unwrap()
                        .options
                );
            }
            let error = peers
                .commit()
                .expect_err("different admitted meanings must not commit");
            assert!(
                error.contains("IncompatibleSetup"),
                "case {index} must fail explicit identity admission: {error}"
            );
            assert!(peers.host.committed_schedule().is_err());
            assert!(peers.join.committed_schedule().is_err());
            assert!(peers
                .host
                .archive_snapshots()
                .unwrap()
                .iter()
                .all(|(_, snapshot)| snapshot.progress.is_none()));
            assert!(peers
                .join
                .archive_snapshots()
                .unwrap()
                .iter()
                .all(|(_, snapshot)| snapshot.progress.is_none()));
            assert!(!peers.host.native_completed() && !peers.join.native_completed());
            peers.pristine();
            let began = Instant::now();
            let _ = peers.finish(&[], &[]); // The original disconnect is retained; both owners still join.
            assert!(began.elapsed() < DEADLINE);
        }
    }
}
