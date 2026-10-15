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
    multiplayer_group_rooms::{GroupRoomMember, GroupRoomPhase},
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
const PREROLL: [i64; 4] = [10_000_000, 25_000_000, 40_000_000, 55_000_000];

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
fn connected_with_identities(case: &str, identities: &[Vec<u8>]) -> Vec<Peer> {
    let config = options(case);
    let (mut peers, mut roster, deadlines) = collecting_with_identities(&config, identities);
    frozen_partial_ready(&mut peers, &mut roster, &deadlines);
    commit_last_ready(&mut peers, &mut roster);
    peers
}

fn collecting_with_identities(
    config: &WebTransportOptions,
    identities: &[Vec<u8>],
) -> (Vec<Peer>, Vec<GroupRoomMember>, Vec<i64>) {
    assert!((2..=4).contains(&identities.len()));
    let mut peers = Vec::new();
    let mut roster = Vec::<GroupRoomMember>::new();
    let mut deadlines = Vec::new();
    for (index, identity) in identities.iter().enumerate() {
        let first = 11 + index as u32 * 10;
        let players = [PlayerId(first), PlayerId(first + 1)];
        peers.push(Peer::connect_admitted(
            config.clone(),
            identity,
            &players,
            PREROLL[index],
        ));
        // Admit one host at a time and observe its actual ID and first deadline
        // before waiting for the exact ordered snapshot at every current peer.
        pump(&mut peers, "new room admission", |peers| {
            let session = peers.last().unwrap().owner().session();
            session.participant().is_some() && session.room().is_some()
        });
        let session = peers[index].owner().session();
        let id = session.participant().unwrap();
        assert_ne!(id.0, 0);
        assert!(roster.iter().all(|member| member.id != id));
        deadlines.push(
            session
                .room()
                .unwrap()
                .deadline_ns
                .expect("collecting room must retain its waiting deadline"),
        );
        roster.push(GroupRoomMember {
            id,
            players: players.to_vec(),
            prepared: false,
        });
        pump(&mut peers, "exact collecting roster after Join", |peers| {
            peers.iter().all(|peer| {
                peer.owner().session().room().is_some_and(|room| {
                    room.phase == GroupRoomPhase::Collecting && room.members == roster.as_slice()
                })
            })
        });
        for (index, peer) in peers.iter_mut().enumerate() {
            let room = peer.owner().session().room().unwrap();
            assert_eq!(room.identity, identities[index].as_slice());
            assert_eq!(room.deadline_ns, Some(deadlines[index]));
            assert!(peer.schedule.is_none());
            assert!(peer
                .owner_mut()
                .publish_progress(&members(11 + index as u32 * 10, 1), false)
                .is_err());
        }
    }
    (peers, roster, deadlines)
}

fn frozen_partial_ready(peers: &mut [Peer], roster: &mut [GroupRoomMember], deadlines: &[i64]) {
    // RoomClientSession refuses these calls before a Seal frame is queued.
    // This proves local API authority, not forged-wire/server enforcement.
    for peer in &mut peers[1..] {
        let error = peer
            .owner_mut()
            .request_seal()
            .expect_err("every joiner must be locally refused permission to seal");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
    peers[0].owner_mut().request_seal().unwrap();
    pump(peers, "frozen complete roster", |peers| {
        peers.iter().all(|peer| {
            peer.owner().session().room().is_some_and(|room| {
                room.phase == GroupRoomPhase::Frozen && room.members == &*roster
            })
        })
    });
    // Each positive partial bitmap must reach every peer before the next Ready.
    // The initial Frozen bitmap is all false, and the last Ready is held back.
    for ready_count in 0..peers.len() {
        if ready_count > 0 {
            peers[ready_count - 1].owner_mut().request_ready().unwrap();
            roster[ready_count - 1].prepared = true;
            pump(peers, "exact partial Ready bitmap", |peers| {
                peers.iter().all(|peer| {
                    peer.owner().session().room().is_some_and(|room| {
                        room.phase == GroupRoomPhase::Frozen && room.members == &*roster
                    })
                })
            });
        }
        for (index, peer) in peers.iter_mut().enumerate() {
            let room = peer.owner().session().room().unwrap();
            assert_eq!(room.phase, GroupRoomPhase::Frozen);
            assert_eq!(room.members, &*roster);
            assert_eq!(room.deadline_ns, Some(deadlines[index]));
            assert!(peer.schedule.is_none());
            assert!(peer
                .owner_mut()
                .publish_progress(&members(11 + index as u32 * 10, 1), false)
                .is_err());
        }
    }
}

fn commit_last_ready(peers: &mut [Peer], roster: &mut [GroupRoomMember]) {
    peers
        .last_mut()
        .unwrap()
        .owner_mut()
        .request_ready()
        .unwrap();
    roster.last_mut().unwrap().prepared = true;
    pump(
        peers,
        "prepared measured clock and committed start",
        |peers| {
            peers.iter().all(|peer| {
                peer.schedule.is_some()
                    && peer.owner().session().room().is_some_and(|room| {
                        room.phase == GroupRoomPhase::Prepared
                            && room.members == &*roster
                            && room.deadline_ns.is_none()
                    })
            })
        },
    );
    for (index, peer) in peers.iter_mut().enumerate() {
        let room = peer.owner().session().room().unwrap();
        assert_eq!(room.phase, GroupRoomPhase::Prepared);
        assert_eq!(room.members, &*roster);
        assert_eq!(room.deadline_ns, None);
        let schedule = peer.schedule.unwrap();
        assert!(schedule.target_ns > 0);
        assert!(schedule.song_target_ns > 0);
        assert_eq!(
            schedule.song_target_ns.checked_sub(schedule.target_ns),
            Some(PREROLL[index])
        );
        assert!(schedule.uncertainty_ns <= StartPolicy::default().max_uncertainty_ns);
        assert_eq!(peer.owner_mut().take_schedule().unwrap(), None);
    }
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

fn complete_progress_and_drain(mut peers: Vec<Peer>) {
    let ids: Vec<_> = peers
        .iter()
        .map(|peer| peer.owner().session().participant().unwrap())
        .collect();
    for tick in 1..=3 {
        let final_prefix = tick == 3;
        let prefixes: Vec<_> = (0..peers.len())
            .map(|index| members(11 + index as u32 * 10, tick))
            .collect();
        for (peer, prefix) in peers.iter_mut().zip(&prefixes) {
            peer.owner_mut()
                .publish_progress(prefix, final_prefix)
                .unwrap();
        }
        pump(&mut peers, "all directed exact ordered prefixes", |peers| {
            peers.iter().enumerate().all(|(receiver, peer)| {
                ids.iter().enumerate().all(|(sender, &id)| {
                    receiver == sender
                        || peer.owner().peer_progress(id).is_some_and(|prefix| {
                            prefix.sequence == tick
                                && prefix.final_prefix == final_prefix
                                && prefix.members == prefixes[sender]
                        })
                })
            })
        });
    }
    pump(
        &mut peers,
        "all genuine final application acknowledgements",
        |peers| peers.iter().all(|peer| peer.owner().progress_complete()),
    );
    for (index, peer) in peers.iter().enumerate() {
        assert!(peer.owner().local_final_written());
        assert!(peer.owner().local_final_acknowledged());
        for (sender, &id) in ids.iter().enumerate() {
            if sender != index {
                let prefix = peer.owner().peer_progress(id).unwrap();
                assert_eq!(prefix.sequence, 3);
                assert!(prefix.final_prefix);
                assert_eq!(prefix.members, members(11 + sender as u32 * 10, 3));
                assert!(peer.owner().peer_final_ack_written(id));
            }
        }
        assert!(
            !peer.owner().drain_complete(),
            "final completion does not fabricate drain ACK"
        );
    }
    let drain_requests: Vec<_> = peers
        .iter_mut()
        .map(|peer| peer.owner_mut().request_drain())
        .collect();
    for result in drain_requests {
        result.unwrap();
    }
    pump(&mut peers, "actual drain acknowledgement", |peers| {
        peers.iter().all(|peer| peer.owner().drain_complete())
    });
    let inert_steps: Vec<_> = peers.iter_mut().map(Peer::step).collect();
    let closes: Vec<_> = peers.iter_mut().map(Peer::close).collect();
    for result in inert_steps {
        assert!(!result.unwrap(), "drained driver moves no new bytes");
    }
    for result in closes {
        result.expect("bounded HTTP/3 owner close failed");
    }
    assert!(peers
        .iter()
        .all(|peer| peer.io.is_none() && peer.retired.is_none()));
}

#[test]
#[ignore = "requires coordinator-owned local HTTP/3 relay and ephemeral valid PKI"]
fn real_room_roster_readiness_start_ordered_progress_finals_and_drain() {
    complete_progress_and_drain(connected("complete"));
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

    #[derive(Clone, Copy)]
    enum RetirementPhase {
        Collecting,
        PartialReady,
        Committed,
    }
    #[derive(Clone, Copy)]
    enum Departure {
        Leave,
        Disconnect,
    }

    fn assert_fenced(peer: &mut Peer, index: usize, first: &io::Error) {
        let prefix = members(11 + index as u32 * 10, 1);
        let errors = [
            peer.owner_mut().configure_frame_wait(1).unwrap_err(),
            peer.owner_mut().request_seal().unwrap_err(),
            peer.owner_mut().request_ready().unwrap_err(),
            peer.owner_mut().request_leave().unwrap_err(),
            peer.owner_mut().request_drain().unwrap_err(),
            peer.owner_mut()
                .publish_progress(&prefix, false)
                .unwrap_err(),
            peer.owner_mut()
                .publish_progress(&prefix, true)
                .unwrap_err(),
            peer.owner_mut().take_schedule().unwrap_err(),
            peer.step().unwrap_err(),
        ];
        for error in errors {
            assert_eq!(error.kind(), first.kind());
            assert_eq!(error.to_string(), first.to_string());
        }
    }

    fn retirement_and_replacement(case: &str, phase: RetirementPhase, departure: Departure) {
        let config = options(case);
        let selected = Selected::default();
        let players = [
            [PlayerId(11), PlayerId(12)],
            [PlayerId(21), PlayerId(22)],
            [PlayerId(31), PlayerId(32)],
        ];
        let identities: Vec<_> = players
            .iter()
            .map(|players| admitted_identity(selected, players))
            .collect();
        assert!(identities.iter().all(|identity| identity == &identities[0]));
        let replacement_identities: Vec<_> = players
            .iter()
            .map(|players| {
                admitted_identity(
                    Selected {
                        seed: selected.seed + 1,
                        ..selected
                    },
                    players,
                )
            })
            .collect();
        assert!(replacement_identities
            .iter()
            .all(|identity| identity == &replacement_identities[0]));
        assert_ne!(identities[0], replacement_identities[0]);
        let (mut old, mut roster, deadlines) = collecting_with_identities(&config, &identities);
        if !matches!(phase, RetirementPhase::Collecting) {
            frozen_partial_ready(&mut old, &mut roster, &deadlines);
        }
        if matches!(phase, RetirementPhase::Committed) {
            commit_last_ready(&mut old, &mut roster);
        }
        let (expected_phase, prepared) = match phase {
            RetirementPhase::Collecting => (GroupRoomPhase::Collecting, [false, false, false]),
            RetirementPhase::PartialReady => (GroupRoomPhase::Frozen, [true, true, false]),
            RetirementPhase::Committed => (GroupRoomPhase::Prepared, [true, true, true]),
        };
        assert_eq!(
            roster
                .iter()
                .map(|member| member.prepared)
                .collect::<Vec<_>>(),
            prepared
        );
        let schedules: Vec<_> = old.iter().map(|peer| peer.schedule).collect();
        let assert_history = |index: usize, peer: &Peer| {
            let owner = peer.owner();
            assert_eq!(owner.session().participant(), Some(roster[index].id));
            let room = owner.session().room().unwrap();
            assert_eq!(room.identity, identities[index].as_slice());
            assert_eq!(room.members, roster.as_slice());
            assert_eq!(room.phase, expected_phase);
            assert_eq!(
                room.deadline_ns,
                if matches!(phase, RetirementPhase::Committed) {
                    None
                } else {
                    Some(deadlines[index])
                }
            );
            // A delivered committed schedule remains history after failure.
            assert_eq!(peer.schedule, schedules[index]);
            assert_eq!(
                peer.schedule.is_some(),
                matches!(phase, RetirementPhase::Committed)
            );
            assert!(!owner.local_final_written());
            assert!(!owner.local_final_acknowledged());
            assert!(!owner.progress_complete());
            assert!(!owner.drain_complete());
            for member in &roster {
                assert!(owner.peer_progress(member.id).is_none());
                assert!(!owner.peer_final_ack_written(member.id));
            }
        };
        for (index, peer) in old.iter().enumerate() {
            assert_history(index, peer);
            assert!(!peer.owner().session().leave_requested());
            assert!(!peer.owner().session().leave_written());
        }

        let deadline = Instant::now() + WAIT;
        match departure {
            Departure::Leave => {
                old[1].owner_mut().request_leave().unwrap();
                while !old[1].owner().session().leave_written() {
                    assert!(
                        Instant::now() < deadline,
                        "middle joiner Leave did not settle"
                    );
                    old[1].step().unwrap();
                }
            }
            Departure::Disconnect => old[1].owner_mut().stop(),
        }
        assert_eq!(
            old[1].owner().session().leave_requested(),
            matches!(departure, Departure::Leave)
        );
        assert_eq!(
            old[1].owner().session().leave_written(),
            matches!(departure, Departure::Leave)
        );
        let own_fence = old[1].owner_mut().request_ready().unwrap_err();
        assert_fenced(&mut old[1], 1, &own_fence);
        assert_history(1, &old[1]);
        old[1].retired = Some(old[1].io.take().unwrap().into_stream());
        let disconnect_close = if matches!(departure, Departure::Disconnect) {
            // Actual bounded FIN/connection shutdown drives the runtime. Local
            // Stop or Drop alone would not establish transport disconnection.
            let started = Instant::now();
            let result = old[1].retired.as_mut().unwrap().finish(CLOSE);
            Some((result, started.elapsed()))
        } else {
            None
        };

        let mut first_errors = [None, None, None];
        while first_errors[0].is_none() || first_errors[2].is_none() {
            assert!(
                Instant::now() < deadline,
                "both survivors must independently observe actual room retirement"
            );
            if matches!(departure, Departure::Leave) {
                // A completed write is not a QUIC-driver flush. Retain and
                // drive this stream until both survivors fail on their own I/O.
                old[1]
                    .retired
                    .as_ref()
                    .unwrap()
                    .idle(Duration::from_millis(1));
            }
            for index in [0, 2] {
                if first_errors[index].is_none() {
                    if let Err(error) = old[index].step() {
                        first_errors[index] = Some(error);
                    }
                }
            }
        }
        for index in [0, 2] {
            let first = first_errors[index].as_ref().unwrap();
            assert!(
                matches!(
                    first.kind(),
                    io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::Other
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::ConnectionAborted
                        | io::ErrorKind::BrokenPipe
                ),
                "expected actual non-timeout transport retirement, got {first}"
            );
            assert_fenced(&mut old[index], index, first);
            assert_history(index, &old[index]);
            assert!(!old[index].owner().session().leave_requested());
            assert!(!old[index].owner().session().leave_written());
        }

        // Reuse this exact configuration/URL while both failed survivor owners
        // and the departing stream still exist, with genuinely different metadata.
        let (mut replacement, mut new_roster, new_deadlines) =
            collecting_with_identities(&config, &replacement_identities);
        assert!(old[0].io.is_some() && old[2].io.is_some() && old[1].retired.is_some());
        for member in &new_roster {
            assert_ne!(member.id.0, 0);
            assert!(roster.iter().all(|old_member| old_member.id != member.id));
        }
        for index in [0, 2] {
            assert_history(index, &old[index]);
            assert_fenced(
                &mut old[index],
                index,
                first_errors[index].as_ref().unwrap(),
            );
        }
        let started = Instant::now();
        let closes: Vec<_> = old.iter_mut().map(Peer::close).collect();
        let elapsed = started.elapsed();
        // Attempt every retained close before checking any close result. A
        // reset may mask a later internal idle timeout in drain.and(closed);
        // retain the original public result and reject returned timeouts only.
        for result in &closes {
            if let Err(error) = result {
                assert_ne!(error.kind(), io::ErrorKind::TimedOut, "old close: {error}");
            }
        }
        assert!(
            elapsed < WAIT,
            "all old close attempts exceeded their bound"
        );
        if let Some((first_close, elapsed)) = disconnect_close {
            assert!(elapsed < WAIT, "transport shutdown exceeded its bound");
            match (first_close, &closes[1]) {
                (Ok(()), Ok(())) => {}
                (Err(first), Err(repeated)) => {
                    assert_ne!(first.kind(), io::ErrorKind::TimedOut, "disconnect: {first}");
                    assert_eq!(repeated.kind(), first.kind());
                    assert_eq!(repeated.to_string(), first.to_string());
                }
                (first, repeated) => {
                    panic!("transport finish result changed: {first:?}, {repeated:?}")
                }
            }
        }
        assert!(old
            .iter()
            .all(|peer| peer.io.is_none() && peer.retired.is_none()));
        frozen_partial_ready(&mut replacement, &mut new_roster, &new_deadlines);
        commit_last_ready(&mut replacement, &mut new_roster);
        complete_progress_and_drain(replacement);
    }

    #[test]
    #[ignore = "requires local HTTP/3 relay and ephemeral valid PKI; actual selected room retirement"]
    fn real_three_host_collecting_leave_releases_room_and_replacement_survives() {
        retirement_and_replacement(
            "collecting_leave",
            RetirementPhase::Collecting,
            Departure::Leave,
        );
    }

    #[test]
    #[ignore = "requires local HTTP/3 relay and ephemeral valid PKI; actual selected room retirement"]
    fn real_three_host_partial_ready_leave_releases_room_and_replacement_survives() {
        retirement_and_replacement(
            "partial_ready_leave",
            RetirementPhase::PartialReady,
            Departure::Leave,
        );
    }

    #[test]
    #[ignore = "requires local HTTP/3 relay and ephemeral valid PKI; actual selected room retirement"]
    fn real_three_host_committed_leave_releases_room_and_replacement_survives() {
        retirement_and_replacement(
            "committed_leave",
            RetirementPhase::Committed,
            Departure::Leave,
        );
    }

    #[test]
    #[ignore = "requires local HTTP/3 relay and ephemeral valid PKI; actual selected room retirement"]
    fn real_three_host_collecting_disconnect_releases_room_and_replacement_survives() {
        retirement_and_replacement(
            "collecting_disconnect",
            RetirementPhase::Collecting,
            Departure::Disconnect,
        );
    }

    #[test]
    #[ignore = "requires local HTTP/3 relay and ephemeral valid PKI; actual selected room retirement"]
    fn real_three_host_partial_ready_disconnect_releases_room_and_replacement_survives() {
        retirement_and_replacement(
            "partial_ready_disconnect",
            RetirementPhase::PartialReady,
            Departure::Disconnect,
        );
    }

    #[test]
    #[ignore = "requires local HTTP/3 relay and ephemeral valid PKI; actual selected room retirement"]
    fn real_three_host_committed_disconnect_releases_room_and_replacement_survives() {
        retirement_and_replacement(
            "committed_disconnect",
            RetirementPhase::Committed,
            Departure::Disconnect,
        );
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
        complete_progress_and_drain(connected_with_identities("selected_complete", &identities));
    }

    #[test]
    #[ignore = "requires local HTTP/3 relay and ephemeral valid PKI; admitted three-host selected policy"]
    fn real_three_host_room_roster_readiness_start_ordered_progress_finals_and_drain() {
        let selected = Selected::default();
        let identities = [
            admitted_identity(selected, &[PlayerId(11), PlayerId(12)]),
            admitted_identity(selected, &[PlayerId(21), PlayerId(22)]),
            admitted_identity(selected, &[PlayerId(31), PlayerId(32)]),
        ];
        assert!(identities.iter().all(|identity| identity == &identities[0]));
        complete_progress_and_drain(connected_with_identities(
            "three_host_complete",
            &identities,
        ));
    }

    #[test]
    #[ignore = "requires local HTTP/3 relay and ephemeral valid PKI; admitted four-host selected policy"]
    fn real_four_host_room_roster_readiness_start_ordered_progress_finals_and_drain() {
        let selected = Selected::default();
        let identities = [
            admitted_identity(selected, &[PlayerId(11), PlayerId(12)]),
            admitted_identity(selected, &[PlayerId(21), PlayerId(22)]),
            admitted_identity(selected, &[PlayerId(31), PlayerId(32)]),
            admitted_identity(selected, &[PlayerId(41), PlayerId(42)]),
        ];
        assert!(identities.iter().all(|identity| identity == &identities[0]));
        complete_progress_and_drain(connected_with_identities("four_host_complete", &identities));
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
