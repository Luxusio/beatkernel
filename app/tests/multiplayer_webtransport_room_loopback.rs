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
        Self::connect_admitted(config, &identity(seed), players, preroll)
    }
    fn connect_admitted(
        config: WebTransportOptions,
        identity: &[u8],
        players: &[PlayerId],
        preroll: i64,
    ) -> Self {
        let stop = AtomicBool::new(false);
        let io = WebTransportEndpoint::prepare(&config)
            .unwrap()
            .connect_room_play(
                identity,
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
    connected_with_identities(case, &[identity(41), identity(41)])
}
fn connected_with_identities(case: &str, identities: &[Vec<u8>; 2]) -> Vec<Peer> {
    let config = options(case);
    let mut peers = vec![Peer::connect_admitted(
        config.clone(),
        &identities[0],
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
    peers.push(Peer::connect_admitted(
        config,
        &identities[1],
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

// Selected metadata comes from actual pristine cohort members. These transport
// owners prove the wire/room lifecycle, separately from native output completion.
mod selected_policy_rooms {
    use super::*;
    use beatkernel::{
        audio::{AudioFormat, PcmLimits, SampleBank},
        input::DeviceId,
        judge::{JudgeGrade, JudgeWindow},
        time::{Duration as SongDuration, Timestamp},
    };
    use beatkernel_bms::{BmsGaugeKind, BmsJudgment};
    use beatkernel_bms_runtime::{
        competition_live::CompetitionOptions,
        native_cohort_setup::{
            prepare_audio_cohort_with_policy, CohortPreparation, PreparedCohort,
        },
        native_group_competition::NativeGroupCompetition,
        native_judge::{prepare_policy_competition_identity, prepare_policy_header},
        play_policy::{ClassifiedWindow, ResolvedPlayPolicy},
        replay_playback::decode_section_setup,
        PreparedBms,
    };
    use std::collections::BTreeMap;

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
    fn policy(source: &beatkernel_bms::BmsChart, selected: Selected) -> ResolvedPlayPolicy {
        ResolvedPlayPolicy::bms(
            source,
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
        .unwrap()
    }
    fn prepared(
        selected: Selected,
        players: &[PlayerId],
    ) -> (PreparedBms, PreparedCohort, ResolvedPlayPolicy) {
        let source = beatkernel_bms::parse_seeded(
            "#BPM 60\n#TOTAL 320\n#WAV01 note.wav\n#00111:0101\n",
            Default::default(),
            selected.seed,
        )
        .unwrap();
        let policy = policy(&source, selected);
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
        let assignments: Vec<_> = players
            .iter()
            .enumerate()
            .map(|(slot, &player)| (player, DeviceId(31 + slot as u64)))
            .collect();
        let bindings = BTreeMap::from([(0x11, 7)]);
        let cohort = prepare_audio_cohort_with_policy(
            &prepared,
            &assignments,
            &CompetitionOptions::default(),
            &CohortPreparation {
                host: ClockDomainId(99),
                output: ClockDomainId(2),
                early: 7,
                late: 9,
                offset: selected.offset,
                preroll: 0,
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
        (prepared, cohort, policy)
    }
    fn admitted_identity(selected: Selected, players: &[PlayerId]) -> Vec<u8> {
        let (prepared, cohort, policy) = prepared(selected, players);
        let policies: Vec<_> = players.iter().map(|&player| (player, &policy)).collect();
        // This public method validates every selected member and identity before
        // its network=None branch. No endpoint or fabricated metadata is needed.
        assert!(NativeGroupCompetition::prepare_with_policies(
            &CompetitionOptions::default(),
            &prepared.source,
            &cohort.configs,
            &policies,
            ClockDomainId(17),
            Timestamp::from_nanos(selected.start),
            selected.seed,
            Some(Timestamp::from_nanos(selected.end)),
            0,
        )
        .unwrap()
        .is_none());
        let mut admitted = None;
        for (member, state) in cohort.configs.iter().zip(&cohort.states) {
            assert!(member.judge.effective_song_time().is_none());
            assert!(state.capture.is_none());
            assert_eq!(state.gauge.profile(), policy.gauge());
            let header = prepare_policy_header(
                &prepared.source,
                &member.judge,
                &policy,
                ClockDomainId(17),
                Timestamp::from_nanos(selected.start),
                selected.seed,
                Some(Timestamp::from_nanos(selected.end)),
            )
            .unwrap();
            let setup = decode_section_setup(&header.options).unwrap();
            assert_eq!(setup.judgments.as_ref(), policy.judgments());
            assert_eq!(setup.gauge, *policy.gauge());
            assert_eq!(setup.start, Timestamp::from_nanos(selected.start));
            assert_eq!(setup.end, Some(Timestamp::from_nanos(selected.end)));
            assert_eq!(setup.chart_seed, selected.seed);
            // The helper builds the canonical inner header with end=None and
            // wraps the actual finite endpoint exactly once.
            let identity = prepare_policy_competition_identity(
                &prepared.source,
                &member.judge,
                &policy,
                ClockDomainId(17),
                Timestamp::from_nanos(selected.start),
                selected.seed,
                Some(Timestamp::from_nanos(selected.end)),
            )
            .unwrap();
            if let Some(expected) = &admitted {
                assert_eq!(&identity, expected);
            } else {
                admitted = Some(identity);
            }
        }
        admitted.unwrap()
    }

    #[test]
    fn selected_room_member_admission_refuses_heterogeneous_class_with_identical_gauge_before_io() {
        let selected = Selected::default();
        let players = [PlayerId(11), PlayerId(12)];
        let (prepared, cohort, selected_policy) = prepared(selected, &players);
        let other = policy(
            &prepared.source,
            Selected {
                class: BmsJudgment::PGreat,
                ..selected
            },
        );
        assert_eq!(selected_policy.judge(), other.judge());
        assert_eq!(selected_policy.gauge(), other.gauge());
        let result = NativeGroupCompetition::prepare_with_policies(
            &CompetitionOptions::default(),
            &prepared.source,
            &cohort.configs,
            &[(players[0], &selected_policy), (players[1], &other)],
            ClockDomainId(17),
            Timestamp::from_nanos(selected.start),
            selected.seed,
            Some(Timestamp::from_nanos(selected.end)),
            0,
        );
        let error = result
            .err()
            .expect("actual heterogeneous selected members must refuse");
        assert!(error.to_string().contains("different competition policies"));
        assert!(cohort
            .configs
            .iter()
            .all(|member| member.judge.effective_song_time().is_none()));
    }

    #[test]
    #[ignore = "requires local HTTP/3 relay and ephemeral valid PKI; genuinely admitted selected policy"]
    fn real_selected_room_members_commit_exchange_prefixes_ack_finals_and_drain() {
        let selected = Selected::default();
        let identities = [
            admitted_identity(selected, &[PlayerId(11), PlayerId(12)]),
            admitted_identity(selected, &[PlayerId(21), PlayerId(22)]),
        ];
        assert_eq!(identities[0], identities[1]);
        let mut peers = connected_with_identities("selected_complete", &identities);
        let ids: Vec<_> = peers
            .iter()
            .map(|peer| peer.owner().session().participant().unwrap())
            .collect();
        for tick in 1..=2 {
            let prefixes = [members(11, tick), members(21, tick)];
            for (peer, prefix) in peers.iter_mut().zip(&prefixes) {
                peer.owner_mut().publish_progress(prefix, false).unwrap();
            }
            pump(&mut peers, "selected exact ordered prefixes", |peers| {
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
        pump(&mut peers, "selected finals and actual ACKs", |peers| {
            peers.iter().all(|peer| peer.owner().progress_complete())
        });
        for (index, peer) in peers.iter().enumerate() {
            let prefix = peer.owner().peer_progress(ids[1 - index]).unwrap();
            assert_eq!(prefix.sequence, 3);
            assert!(prefix.final_prefix);
            assert_eq!(prefix.members, finals[1 - index]);
            assert!(peer.owner().local_final_written());
            assert!(peer.owner().local_final_acknowledged());
            assert!(peer.owner().peer_final_ack_written(ids[1 - index]));
            assert!(!peer.owner().drain_complete());
        }
        for peer in &mut peers {
            peer.owner_mut().request_drain().unwrap();
        }
        pump(&mut peers, "selected actual drain ACKs", |peers| {
            peers.iter().all(|peer| peer.owner().drain_complete())
        });
        for peer in &mut peers {
            assert!(!peer.step().unwrap());
            peer.close()
                .expect("selected HTTP/3 owners must close boundedly");
        }
    }

    #[test]
    #[ignore = "requires local HTTP/3 relay and ephemeral valid PKI; admitted selected peer mismatches"]
    fn real_selected_room_peers_refuse_class_gauge_profile_section_and_seed_before_admission() {
        let selected = Selected::default();
        let canonical = admitted_identity(selected, &[PlayerId(1), PlayerId(2)]);
        for (case, foreign) in [
            (
                "class",
                Selected {
                    class: BmsJudgment::PGreat,
                    ..selected
                },
            ),
            (
                "gauge",
                Selected {
                    gauge: BmsGaugeKind::Hazard,
                    ..selected
                },
            ),
            (
                "profile",
                Selected {
                    offset: selected.offset + 1,
                    ..selected
                },
            ),
            (
                "start",
                Selected {
                    start: selected.start + 1,
                    ..selected
                },
            ),
            (
                "end",
                Selected {
                    end: selected.end - 1,
                    ..selected
                },
            ),
            (
                "seed",
                Selected {
                    seed: selected.seed + 1,
                    ..selected
                },
            ),
        ] {
            let foreign_identity = admitted_identity(foreign, &[PlayerId(9), PlayerId(10)]);
            assert_ne!(canonical, foreign_identity);
            let config = options(&format!("selected_{case}"));
            let mut host =
                Peer::connect_admitted(config.clone(), &canonical, &[PlayerId(1), PlayerId(2)], 0);
            pump(
                std::slice::from_mut(&mut host),
                "selected original admission",
                |peers| {
                    peers[0]
                        .owner()
                        .session()
                        .room()
                        .is_some_and(|room| room.members.len() == 1)
                },
            );
            let original = host.owner().session().participant();
            let mut foreign =
                Peer::connect_admitted(config, &foreign_identity, &[PlayerId(9), PlayerId(10)], 0);
            let deadline = Instant::now() + WAIT;
            let error = loop {
                assert!(
                    Instant::now() < deadline,
                    "selected {case} was not explicitly refused"
                );
                host.step().unwrap();
                if let Err(error) = foreign.step() {
                    break error;
                }
            };
            assert_ne!(
                error.kind(),
                io::ErrorKind::TimedOut,
                "timeout is not selected identity refusal"
            );
            assert_eq!(foreign.owner().session().participant(), None);
            assert!(foreign.owner().session().room().is_none());
            assert!(foreign.schedule.is_none());
            assert!(!foreign.owner().progress_complete());
            assert!(!foreign.owner().drain_complete());
            assert_eq!(host.owner().session().participant(), original);
            assert_eq!(host.owner().session().room().unwrap().members.len(), 1);
            assert!(host.schedule.is_none());
            for result in [foreign.close(), host.close()] {
                if let Err(error) = result {
                    assert_ne!(error.kind(), io::ErrorKind::TimedOut);
                }
            }
            assert!(foreign.io.is_none() && foreign.retired.is_none());
            assert!(host.io.is_none() && host.retired.is_none());
        }
    }
}
