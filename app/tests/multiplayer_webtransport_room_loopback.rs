//! Real HTTP/3/BKMR tests against a coordinator-owned local relay and ephemeral PKI.
//! The relay must allow BEATKERNEL_TEST_WEBTRANSPORT_ORIGIN and use --group-hosts 4.
//! No test generates credentials or leaves a server thread running.
#![cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]

use beatkernel::{
    input::CodecLimits,
    replay::{codec::ReplayCodecLimits, ReplayHeader, REPLAY_VERSION},
    time::ClockDomainId,
};
use beatkernel_bms_runtime::{
    local_players::PlayerId,
    multiplayer::{competition_identity, Progress},
    multiplayer_group::MemberProgress,
    multiplayer_group_rooms::GroupRoomPhase,
    multiplayer_room_io::RoomPlayIo,
    multiplayer_start::{StartPolicy, StartRole, StartSchedule},
    multiplayer_webtransport_client::{
        WebTransportEndpoint, WebTransportOptions, WebTransportStream,
    },
};
use std::{
    env, io,
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(10);
const CLOSE: Duration = Duration::from_secs(2);
const PREROLL: [i64; 2] = [10_000_000, 25_000_000];

fn identity(seed: u64) -> Vec<u8> {
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
        ReplayCodecLimits::new(65_536, 1, 4096, CodecLimits::new(64, 0).unwrap()).unwrap(),
    )
    .unwrap()
}

fn options(case: &str) -> WebTransportOptions {
    let base = env::var("BEATKERNEL_TEST_WEBTRANSPORT_URL")
        .expect("real room integration requires BEATKERNEL_TEST_WEBTRANSPORT_URL");
    WebTransportOptions {
        url: format!("{base}_{}_{case}", std::process::id()),
        origin: env::var("BEATKERNEL_TEST_WEBTRANSPORT_ORIGIN")
            .expect("real room integration requires BEATKERNEL_TEST_WEBTRANSPORT_ORIGIN"),
        ca: PathBuf::from(
            env::var_os("BEATKERNEL_TEST_QUIC_CA")
                .expect("real room integration requires BEATKERNEL_TEST_QUIC_CA"),
        ),
        role: StartRole::Join,
    }
}

struct Peer {
    io: Option<RoomPlayIo<WebTransportStream>>,
    retired: Option<WebTransportStream>,
    origin: Instant,
    schedule: Option<StartSchedule>,
}
impl Peer {
    fn connect(config: WebTransportOptions, seed: u64, players: &[PlayerId], preroll: i64) -> Self {
        let stop = AtomicBool::new(false);
        let io = WebTransportEndpoint::prepare(&config)
            .unwrap()
            .connect_room_play(
                &identity(seed),
                players,
                StartPolicy::default(),
                preroll,
                &stop,
                Instant::now() + WAIT,
            )
            .expect("actual HTTP/3 room stream setup failed");
        Self {
            io: Some(io),
            retired: None,
            origin: Instant::now(),
            schedule: None,
        }
    }
    fn owner(&self) -> &RoomPlayIo<WebTransportStream> {
        self.io.as_ref().unwrap()
    }
    fn owner_mut(&mut self) -> &mut RoomPlayIo<WebTransportStream> {
        self.io.as_mut().unwrap()
    }
    fn step(&mut self) -> io::Result<bool> {
        let origin = self.origin;
        let owner = self.io.as_mut().unwrap();
        let moved = owner.step(|| {
            i64::try_from(origin.elapsed().as_nanos())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "elapsed clock overflow"))
        })?;
        if owner.session().leave_written() {
            return Ok(moved);
        }
        if let Some(schedule) = owner.take_schedule()? {
            assert!(
                self.schedule.replace(schedule).is_none(),
                "duplicate committed start"
            );
        }
        Ok(moved)
    }
    fn close(&mut self) -> io::Result<()> {
        if let Some(mut owner) = self.io.take() {
            owner.stop();
            self.retired = Some(owner.into_stream());
        }
        if let Some(mut stream) = self.retired.take() {
            stream.finish(CLOSE)
        } else {
            Ok(())
        }
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn pump(peers: &mut [Peer], label: &str, done: impl Fn(&[Peer]) -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done(peers) {
        assert!(Instant::now() < deadline, "timed out waiting for {label}");
        for peer in peers.iter_mut() {
            peer.step()
                .unwrap_or_else(|error| panic!("{label}: {error}"));
        }
    }
}

fn connected(case: &str) -> Vec<Peer> {
    let config = options(case);
    let mut peers = vec![Peer::connect(
        config.clone(),
        41,
        &[PlayerId(11), PlayerId(12)],
        PREROLL[0],
    )];
    // Observe first admission before adding the next peer, making room sealing
    // authority deterministic without sleeping for a guessed network delay.
    pump(&mut peers, "first room admission", |peers| {
        peers[0].owner().session().participant().is_some()
    });
    assert!(peers[0]
        .owner_mut()
        .publish_progress(&members(11, 1), false)
        .is_err());
    assert!(peers[0].schedule.is_none());
    peers.push(Peer::connect(
        config,
        41,
        &[PlayerId(21), PlayerId(22)],
        PREROLL[1],
    ));
    pump(&mut peers, "complete collecting roster", |peers| {
        peers.iter().all(|peer| {
            peer.owner()
                .session()
                .room()
                .is_some_and(|room| room.members.len() == 2)
        })
    });
    let ids: Vec<_> = peers
        .iter()
        .map(|peer| peer.owner().session().participant().unwrap())
        .collect();
    for peer in &peers {
        let room = peer.owner().session().room().unwrap();
        assert_eq!(room.phase, GroupRoomPhase::Collecting);
        assert_eq!(
            room.members
                .iter()
                .map(|member| member.id)
                .collect::<Vec<_>>(),
            ids
        );
        assert_eq!(room.members[0].players, vec![PlayerId(11), PlayerId(12)]);
        assert_eq!(room.members[1].players, vec![PlayerId(21), PlayerId(22)]);
    }
    assert!(
        peers[1].owner_mut().request_seal().is_err(),
        "joiner cannot seal"
    );
    peers[0].owner_mut().request_seal().unwrap();
    pump(&mut peers, "frozen complete roster", |peers| {
        peers.iter().all(|peer| {
            peer.owner()
                .session()
                .room()
                .is_some_and(|room| room.phase == GroupRoomPhase::Frozen)
        })
    });
    // Both actual Ready writes are needed before Prepared or a start schedule.
    peers[0].owner_mut().request_ready().unwrap();
    pump(&mut peers, "first Ready receipt", |peers| {
        peers.iter().all(|peer| {
            peer.owner()
                .session()
                .room()
                .is_some_and(|room| room.members[0].prepared)
        })
    });
    assert!(peers.iter().all(|peer| peer.schedule.is_none()));
    peers[1].owner_mut().request_ready().unwrap();
    pump(
        &mut peers,
        "prepared measured clock and committed start",
        |peers| peers.iter().all(|peer| peer.schedule.is_some()),
    );
    for (index, peer) in peers.iter().enumerate() {
        let room = peer.owner().session().room().unwrap();
        assert_eq!(room.phase, GroupRoomPhase::Prepared);
        assert!(room.members.iter().all(|member| member.prepared));
        assert_eq!(room.deadline_ns, None);
        let schedule = peer.schedule.unwrap();
        assert!(schedule.target_ns > 0);
        assert_eq!(
            schedule.song_target_ns.checked_sub(schedule.target_ns),
            Some(PREROLL[index])
        );
        assert!(schedule.uncertainty_ns <= StartPolicy::default().max_uncertainty_ns);
    }
    peers
}

fn members(first: u32, tick: u64) -> Vec<MemberProgress> {
    (0..2)
        .map(|slot| MemberProgress {
            player: PlayerId(first + slot),
            progress: Progress {
                song_ns: tick as i64 * 1_000_000_000,
                hits: tick * 4,
                misses: tick,
                combo: tick,
                max_combo: tick * 2,
            },
        })
        .collect()
}

#[test]
#[ignore = "requires coordinator-owned local HTTP/3 relay and ephemeral valid PKI"]
fn real_room_roster_readiness_start_ordered_progress_finals_and_drain() {
    let mut peers = connected("complete");
    let ids: Vec<_> = peers
        .iter()
        .map(|peer| peer.owner().session().participant().unwrap())
        .collect();
    for tick in 1..=2 {
        let prefixes = [members(11, tick), members(21, tick)];
        for (peer, prefix) in peers.iter_mut().zip(&prefixes) {
            peer.owner_mut().publish_progress(prefix, false).unwrap();
        }
        pump(&mut peers, "ordered exact progress", |peers| {
            peers.iter().enumerate().all(|(index, peer)| {
                peer.owner()
                    .peer_progress(ids[1 - index])
                    .is_some_and(|prefix| {
                        prefix.sequence == tick
                            && !prefix.final_prefix
                            && prefix.members == prefixes[1 - index]
                    })
            })
        });
    }
    let finals = [members(11, 3), members(21, 3)];
    for (peer, prefix) in peers.iter_mut().zip(&finals) {
        peer.owner_mut().publish_progress(prefix, true).unwrap();
    }
    pump(
        &mut peers,
        "both final prefixes and genuine application acknowledgements",
        |peers| peers.iter().all(|peer| peer.owner().progress_complete()),
    );
    for (index, peer) in peers.iter().enumerate() {
        let prefix = peer.owner().peer_progress(ids[1 - index]).unwrap();
        assert_eq!(prefix.sequence, 3);
        assert!(prefix.final_prefix);
        assert_eq!(prefix.members, finals[1 - index]);
        assert!(peer.owner().local_final_written());
        assert!(peer.owner().local_final_acknowledged());
        assert!(peer.owner().peer_final_ack_written(ids[1 - index]));
        assert!(
            !peer.owner().drain_complete(),
            "final completion does not fabricate drain ACK"
        );
    }
    for peer in &mut peers {
        peer.owner_mut().request_drain().unwrap();
    }
    pump(&mut peers, "actual drain acknowledgement", |peers| {
        peers.iter().all(|peer| peer.owner().drain_complete())
    });
    for peer in &mut peers {
        assert!(!peer.step().unwrap(), "drained driver moves no new bytes");
    }
    let results: Vec<_> = peers.iter_mut().map(Peer::close).collect();
    for result in results {
        result.expect("bounded HTTP/3 owner close failed");
    }
}

#[test]
#[ignore = "requires coordinator-owned local HTTP/3 relay and ephemeral valid PKI"]
fn real_room_leave_cancels_cohort_and_fences_committed_owner() {
    let mut peers = connected("cancel");
    peers[0].owner_mut().request_leave().unwrap();
    let deadline = Instant::now() + WAIT;
    while !peers[0].owner().session().leave_written() {
        assert!(Instant::now() < deadline, "Leave write did not settle");
        peers[0].step().unwrap();
    }
    let mut leaving = peers[0].io.take().unwrap();
    assert!(leaving.publish_progress(&members(11, 1), false).is_err());
    assert!(leaving.request_ready().is_err());
    assert!(leaving.request_seal().is_err());
    assert!(leaving.take_schedule().is_err());
    assert!(!leaving.drain_complete());
    peers[0].retired = Some(leaving.into_stream());
    // The successful stream write is not a QUIC-driver flush. Keep the
    // caller-owned current-thread transport runtime running until the other
    // participant observes actual cancellation, without inventing an ACK or
    // closing this connection to manufacture that observation.
    let terminal = loop {
        assert!(
            Instant::now() < deadline,
            "remote cohort did not observe cancellation"
        );
        peers[0]
            .retired
            .as_ref()
            .unwrap()
            .idle(Duration::from_millis(1));
        if let Err(error) = peers[1].step() {
            break error;
        }
    };
    assert!(
        matches!(
            terminal.kind(),
            io::ErrorKind::UnexpectedEof
                | io::ErrorKind::Other
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::BrokenPipe
        ),
        "expected actual transport cancellation, got {terminal}"
    );
    assert!(peers[1]
        .owner_mut()
        .publish_progress(&members(21, 1), false)
        .is_err());
    assert!(peers[1].owner_mut().request_ready().is_err());
    assert!(peers[1].owner_mut().request_seal().is_err());
    assert!(peers[1].owner_mut().take_schedule().is_err());
    assert!(peers[1].step().is_err());
    assert!(!peers[1].owner().drain_complete());
    assert!(!peers[1].owner().progress_complete());
    let closed: Vec<_> = peers.iter_mut().map(Peer::close).collect();
    for result in closed {
        // A cancelled relay can reset an already-closed send stream. That is
        // failure evidence, not a drain receipt; disposal must still settle.
        if let Err(error) = result {
            assert_ne!(
                error.kind(),
                io::ErrorKind::TimedOut,
                "cancel cleanup did not settle: {error}"
            );
        }
    }
    assert!(peers
        .iter()
        .all(|peer| peer.io.is_none() && peer.retired.is_none()));
}

#[test]
#[ignore = "requires coordinator-owned local HTTP/3 relay and ephemeral valid PKI"]
fn real_origin_and_untrusted_ca_refuse_before_room_admission() {
    for case in ["origin", "trust"] {
        let mut config = options(case);
        if case == "origin" {
            config.origin = "https://beatkernel-forbidden-origin.invalid".into();
        } else {
            config.ca =
                PathBuf::from(env::var_os("BEATKERNEL_TEST_QUIC_WRONG_CA").expect(
                    "trust refusal requires an unrelated valid BEATKERNEL_TEST_QUIC_WRONG_CA",
                ));
        }
        let result = WebTransportEndpoint::prepare(&config)
            .unwrap()
            .connect_room_play(
                &identity(41),
                &[PlayerId(1)],
                StartPolicy::default(),
                0,
                &AtomicBool::new(false),
                Instant::now() + WAIT,
            );
        let error = result
            .err()
            .expect("forbidden Origin/untrusted certificate acquired a room stream");
        assert_ne!(
            error.kind(),
            io::ErrorKind::InvalidInput,
            "test must reach real HTTP/3/TLS admission: {error}"
        );
        assert_ne!(
            error.kind(),
            io::ErrorKind::TimedOut,
            "a setup timeout does not prove Origin/TLS refusal: {error}"
        );
    }
}

#[test]
#[ignore = "requires coordinator-owned local HTTP/3 relay and ephemeral valid PKI"]
fn real_incompatible_identity_never_publishes_foreign_roster_or_start() {
    let config = options("identity");
    let mut host = Peer::connect(config.clone(), 41, &[PlayerId(1)], 0);
    pump(
        std::slice::from_mut(&mut host),
        "original admission",
        |peers| {
            peers[0]
                .owner()
                .session()
                .room()
                .is_some_and(|room| room.members.len() == 1)
        },
    );
    let original = host.owner().session().participant();
    let mut foreign = Peer::connect(config, 42, &[PlayerId(9)], 0);
    let deadline = Instant::now() + WAIT;
    loop {
        assert!(
            Instant::now() < deadline,
            "identity mismatch was not refused"
        );
        host.step().unwrap();
        if foreign.step().is_err() {
            break;
        }
    }
    assert_eq!(foreign.owner().session().participant(), None);
    assert!(foreign.schedule.is_none());
    assert_eq!(host.owner().session().participant(), original);
    assert_eq!(host.owner().session().room().unwrap().members.len(), 1);
    assert!(host.schedule.is_none());
    let foreign_close = foreign.close();
    let host_close = host.close();
    for result in [foreign_close, host_close] {
        if let Err(error) = result {
            assert_ne!(
                error.kind(),
                io::ErrorKind::TimedOut,
                "identity-refusal cleanup did not settle: {error}"
            );
        }
    }
    assert!(foreign.io.is_none() && host.io.is_none());
}
