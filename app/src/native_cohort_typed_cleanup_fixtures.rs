//! Actual cohort finalization retains typed native failures through every save effect.
use super::*;
use crate::{
    competition::{Competition, OpponentKind},
    native_section_capture_fixtures::capture,
    native_solo_result_fixtures::result_source,
    play_result::{CompletedLocalPublicationError, CompletedPlayResult},
};
use beatkernel::{input::CodecLimits, replay::codec::ReplayCodecLimits};
use std::{cell::Cell, error::Error, fmt};

#[derive(Debug)]
struct Fault(&'static str);
impl fmt::Display for Fault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}
impl Error for Fault {}

fn fault(label: &'static str) -> (Box<dyn Error>, *const Fault) {
    let error = Box::new(Fault(label));
    let pointer = error.as_ref() as *const Fault;
    (error, pointer)
}

fn assert_original(error: &(dyn Error + 'static), pointer: *const Fault) {
    assert_eq!(error.downcast_ref::<Fault>().unwrap() as *const _, pointer);
}

fn selected(player: PlayerId) -> LiveCompetition {
    let recording = capture(0, None, u64::from(player.0)).into_file();
    let mut competition = Competition::new(recording.header.clone(), 1).unwrap();
    competition
        .add_replay(
            &result_source(false),
            recording,
            ReplayCodecLimits::new(65_536, 32, 4096, CodecLimits::new(4096, 1024).unwrap())
                .unwrap(),
            OpponentKind::Own,
            format!("opponents/member{}.bkr", player.0),
        )
        .unwrap();
    LiveCompetition::from_prepared(player, competition, None, std::time::Duration::from_secs(1))
        .unwrap()
}

fn states(ids: &[PlayerId], with_opponents: bool) -> Vec<PlayerState> {
    ids.iter()
        .map(|&player| PlayerState {
            player,
            capture: Some(capture(0, None, u64::from(player.0))),
            competition: with_opponents.then(|| selected(player)),
            completion: None,
            score: ScoreSummary::default(),
            gauge: crate::gauge::BmsGauge::default(),
            last_song: Timestamp::ZERO,
        })
        .collect()
}

fn destinations(ids: &[PlayerId]) -> Vec<(PlayerId, Option<PathBuf>)> {
    ids.iter()
        .rev()
        .map(|&player| {
            (
                player,
                Some(PathBuf::from(format!("member{}.bkr", player.0))),
            )
        })
        .collect()
}

fn captured_player(capture: &LiveReplayCapture) -> PlayerId {
    PlayerId(
        u32::try_from(
            crate::replay_playback::decode_section_setup(&capture.header().options)
                .unwrap()
                .chart_seed,
        )
        .unwrap(),
    )
}

fn completions(ids: &[PlayerId]) -> Vec<(PlayerId, CompletedPlayResult)> {
    ids.iter()
        .rev()
        .map(|&player| {
            (
                player,
                CompletedPlayResult::from_completed(
                    Timestamp::ZERO,
                    None,
                    &crate::gauge::BmsGauge::default(),
                ),
            )
        })
        .collect()
}

#[test]
fn every_typed_primary_cleanup_and_save_failure_mask_visits_all_members_before_proven_archive() {
    let ids = [PlayerId(99), PlayerId(7), PlayerId(u32::MAX)];
    for mask in 0..16 {
        let proof = completions(&ids);
        let (primary_cause, _) = fault("native output primary");
        let primary = Box::new(CompletedLocalPublicationError {
            results: proof.clone(),
            cause: primary_cause,
        });
        let primary_pointer = primary.as_ref() as *const CompletedLocalPublicationError;
        let outcome = if mask & 1 != 0 {
            Err(primary as Box<dyn Error>)
        } else {
            Ok(Some(proof.clone()))
        };
        let (cleanup_error, cleanup_pointer) = fault("native output cleanup");
        let cleanup = if mask & 2 != 0 {
            Err(cleanup_error)
        } else {
            Ok(())
        };
        let (archive_error, archive_pointer) = fault("archive save");
        let mut calls = Vec::new();
        let saved = Cell::new(0);
        let archived = Cell::new(0);
        let result = finish_cohort_with_results_and_network_and_cleanup(
            outcome,
            cleanup,
            states(&ids, false),
            None,
            destinations(&ids),
            Vec::new(),
            Some(Path::new("entire-session.bkr")),
            |recording, path, failed| {
                let player = captured_player(&recording.unwrap());
                assert_eq!(path, Some(Path::new(&format!("member{}.bkr", player.0))));
                assert_eq!(failed, mask & 3 != 0);
                calls.push(player);
                saved.set(saved.get() + 1);
                if mask & 4 != 0 {
                    Err("member replay save refused".into())
                } else {
                    Ok(())
                }
            },
            |archive, base| {
                assert_eq!(saved.get(), ids.len(), "archive preceded a member save");
                archived.set(archived.get() + 1);
                assert_eq!(base, Some(Path::new("entire-session.bkr")));
                assert_eq!(
                    archive
                        .entries()
                        .iter()
                        .map(|entry| entry.player)
                        .collect::<Vec<_>>(),
                    proof.iter().map(|row| row.0).collect::<Vec<_>>()
                );
                if mask & 8 != 0 {
                    Err(archive_error)
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(calls, ids, "save failure mask {mask}");
        assert_eq!(archived.get(), 1, "proven completion mask {mask}");
        if mask & 1 != 0 {
            let error = result.unwrap_err();
            let retained = error
                .downcast_ref::<CompletedLocalPublicationError>()
                .unwrap();
            assert_eq!(retained as *const _, primary_pointer);
            assert_eq!(retained.results, proof);
            assert_eq!(
                retained.cause.downcast_ref::<Fault>().unwrap().0,
                "native output primary"
            );
        } else if mask & 2 != 0 {
            assert_original(result.unwrap_err().as_ref(), cleanup_pointer);
        } else if mask & 4 != 0 {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("member replay save refused"));
        } else if mask & 8 != 0 {
            assert_original(result.unwrap_err().as_ref(), archive_pointer);
        } else {
            result.unwrap();
        }
    }
}

#[test]
fn failed_start_and_cancelled_prefix_never_invent_completion_and_keep_original_typed_box() {
    let ids = [PlayerId(99), PlayerId(7), PlayerId(u32::MAX)];
    for mask in 0..4 {
        let (primary, primary_pointer) = fault("native start failed");
        let (cleanup, cleanup_pointer) = fault("native stop failed");
        let mut calls = Vec::new();
        let result = finish_cohort_with_results_and_network_and_cleanup(
            if mask & 1 != 0 {
                Err(primary)
            } else {
                Ok(None)
            },
            if mask & 2 != 0 { Err(cleanup) } else { Ok(()) },
            states(&ids, false),
            None,
            destinations(&ids),
            Vec::new(),
            Some(Path::new("failed-start.bkr")),
            |recording, _, failed| {
                assert_eq!(failed, mask != 0);
                calls.push(captured_player(&recording.unwrap()));
                Ok(())
            },
            |_, _| panic!("an uncompleted prefix cannot publish a completion archive"),
        );
        assert_eq!(calls, ids);
        match mask {
            0 => result.unwrap(),
            2 => assert_original(result.unwrap_err().as_ref(), cleanup_pointer),
            _ => assert_original(result.unwrap_err().as_ref(), primary_pointer),
        }
    }
}

#[test]
fn typed_cleanup_failure_still_saves_every_unambiguous_destination_after_roster_errors() {
    let ids = [7, 99, u32::MAX, 21, 50, 40].map(PlayerId);
    let (cleanup, pointer) = fault("typed native cleanup");
    let mut calls = Vec::new();
    let result = finish_cohort_with_results_and_network_and_cleanup(
        Ok(None),
        Err(cleanup),
        states(&ids, false),
        None,
        vec![
            (ids[0], Some("shared.bkr".into())),
            (ids[1], Some("shared.bkr".into())),
            (ids[2], Some("ambiguous.bkr".into())),
            (ids[2], None),
            (ids[5], Some("last.bkr".into())),
            (ids[4], Some("safe.bkr".into())),
            (PlayerId(1), None),
        ],
        vec!["practice destination preparation failed".into()],
        Some(Path::new("all.bkr")),
        |recording, path, failed| {
            assert!(failed);
            calls.push((
                captured_player(&recording.unwrap()),
                path.unwrap().to_path_buf(),
            ));
            Err("individual publication failed".into())
        },
        |_, _| panic!("roster errors do not prove completion"),
    );
    assert_original(result.unwrap_err().as_ref(), pointer);
    assert_eq!(
        calls,
        [
            (ids[4], PathBuf::from("safe.bkr")),
            (ids[5], PathBuf::from("last.bkr"))
        ]
    );
}

#[test]
fn typed_cleanup_finalization_attempts_real_captures_for_every_supported_cohort_member() {
    for count in [2, MAX_LOCAL_PLAYERS] {
        let ids = (0..count)
            .map(|index| PlayerId(u32::MAX - index as u32 * 17))
            .collect::<Vec<_>>();
        let (cleanup, pointer) = fault("full cohort cleanup");
        let mut calls = Vec::new();
        let result = finish_cohort_with_results_and_network_and_cleanup(
            Ok(None),
            Err(cleanup),
            states(&ids, false),
            None,
            destinations(&ids),
            Vec::new(),
            None,
            |recording, path, failed| {
                assert!(failed);
                let player = captured_player(&recording.unwrap());
                assert_eq!(path, Some(Path::new(&format!("member{}.bkr", player.0))));
                calls.push(player);
                Err("save remains independently fallible".into())
            },
            |_, _| panic!("no base path or completion"),
        );
        assert_original(result.unwrap_err().as_ref(), pointer);
        assert_eq!(calls, ids);
    }
}

#[test]
fn typed_primary_and_cleanup_failures_still_finish_every_actual_selected_opponent_owner() {
    let ids = [PlayerId(99), PlayerId(7), PlayerId(u32::MAX)];
    let source = result_source(false);
    let chart = source.compile().unwrap().chart;
    let (publisher, viewer) = crate::player::channel();
    let mut calls = Vec::new();
    let result = crate::player::with_publisher(publisher, || {
        crate::player::publish_local_chart(&source, &chart, &ids).unwrap();
        assert!(viewer
            .take_latest()
            .unwrap()
            .players
            .iter()
            .all(|row| row.competition.is_none()));
        let (primary, pointer) = fault("failed actual native start");
        let (cleanup, _) = fault("failed actual native cleanup");
        let error = finish_cohort_with_results_and_network_and_cleanup(
            Err(primary),
            Err(cleanup),
            states(&ids, true),
            None,
            destinations(&ids),
            Vec::new(),
            Some(Path::new("failed.bkr")),
            |recording, _, failed| {
                assert!(failed);
                calls.push(captured_player(&recording.unwrap()));
                Err("replay failure cannot skip a later opponent owner".into())
            },
            |_, _| panic!("failed start is not completion"),
        )
        .unwrap_err();
        assert_original(error.as_ref(), pointer);
        Err::<(), String>(error.to_string())
    });
    assert!(result.is_err());
    assert_eq!(calls, ids);
    let final_snapshot = viewer.take_latest().unwrap();
    assert_eq!(final_snapshot.players.len(), ids.len());
    for row in &final_snapshot.players {
        let comparison = row
            .competition
            .as_ref()
            .expect("finish must publish each selected owner");
        assert!(comparison.network.is_none());
        assert_eq!(comparison.ghosts.len(), 1);
        assert_eq!(
            comparison.ghosts[0].label,
            format!("member{}.bkr", row.player.0)
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "real QUIC host requires existing BEATKERNEL_TEST_QUIC_CERT/KEY development TLS environment"]
fn typed_cleanup_failure_still_stops_the_actual_borrowed_cohort_network_owner() {
    use crate::competition_live::NetworkRole;
    use beatkernel::audio::{AudioFormat, PcmLimits, SampleBank};
    use std::{
        net::{Ipv4Addr, UdpSocket},
        time::Duration as WallDuration,
    };

    let source = result_source(false);
    let prepared = PreparedBms {
        compiled: source.compile().unwrap(),
        source,
        bank: SampleBank::new(
            AudioFormat::new(1000, 1).unwrap(),
            PcmLimits::new(64, 256, 4).unwrap(),
        )
        .unwrap(),
        sounds: Vec::new(),
        bgm_commands: Vec::new(),
    };
    let address = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap();
    let mut competition = CompetitionOptions::default();
    competition.network = Some(NetworkRole::Host(address));
    competition.setup_timeout = WallDuration::from_secs(2);
    competition.quic.cert = Some(
        std::env::var_os("BEATKERNEL_TEST_QUIC_CERT")
            .expect("actual network owner requires existing TLS certificate")
            .into(),
    );
    competition.quic.key = Some(
        std::env::var_os("BEATKERNEL_TEST_QUIC_KEY")
            .expect("actual network owner requires existing TLS key")
            .into(),
    );
    let bindings = BTreeMap::from([(0x11, 7)]);
    let ids = [PlayerId(99), PlayerId(7)];
    let mut cohort = prepare_cohort(
        &prepared,
        &[(ids[0], DeviceId(11)), (ids[1], DeviceId(12))],
        &competition,
        &CohortPreparation {
            host: ClockDomainId(1),
            output: ClockDomainId(2),
            early: 0,
            late: 0,
            offset: 0,
            preroll: 0,
            start: Timestamp::ZERO,
            end: None,
            chart_seed: 17,
            bindings: &bindings,
            record_replay: None,
            replay_max_bytes: 0,
            replay_max_records: 0,
        },
    )
    .unwrap();
    let network = cohort.network.as_mut().expect("real QUIC owner");
    let (cleanup, pointer) = fault("native cleanup before network finish");
    let mut saved = 0;
    let error = finish_cohort_with_results_and_network_and_cleanup(
        Ok(None),
        Err(cleanup),
        cohort.states,
        Some(&mut *network),
        destinations(&ids),
        Vec::new(),
        None,
        |capture, _, failed| {
            assert!(capture.is_none());
            assert!(failed);
            saved += 1;
            Ok(())
        },
        |_, _| panic!("unplayed network cancellation has no completed result"),
    )
    .unwrap_err();
    assert_original(error.as_ref(), pointer);
    assert_eq!(saved, ids.len());
    assert_eq!(
        network.finish(&[]).unwrap_err().to_string(),
        "group competition already stopped"
    );
}
