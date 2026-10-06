//! Deferred common cohort finish with offline selected owners and injected saves.
use super::*;
use crate::{
    competition::{Competition, OpponentKind},
    gauge::BmsGauge,
    native_section_capture_fixtures::capture,
    native_solo_result_fixtures::result_source,
    play_result::{CompletedPlayResult, CompletedLocalPublicationError},
    result_archive::{ArchivedScore, encode_archive, decode_archive},
};
use beatkernel::{input::CodecLimits, replay::codec::ReplayCodecLimits};
use std::{cell::RefCell, sync::Arc, error::Error, fmt};
#[derive(Debug)]
struct Fault(Arc<usize>);
impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cohort replay refusal {}", self.0)
    }
}
impl Error for Fault {}
fn selected(id: PlayerId) -> LiveCompetition {
    selected_count(id, 1)
}
fn selected_count(id: PlayerId, count: usize) -> LiveCompetition {
    let file = capture(0, None, id.0 as u64).into_file();
    let mut competition = Competition::new(file.header.clone(), count).unwrap();
    let limits =
        ReplayCodecLimits::new(65536, 32, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    for _ in 0..count {
        competition
            .add_replay(
                &result_source(false),
                file.clone(),
                limits,
                OpponentKind::Own,
                format!("directory\\member{}.bkr", id.0),
            )
            .unwrap();
    }
    LiveCompetition::from_prepared(id, competition, None, std::time::Duration::from_secs(1))
        .unwrap()
}
fn states(count: u32) -> Vec<PlayerState> {
    (0..count)
        .map(|index| {
            let player = PlayerId(u32::MAX - index * 17);
            let hits = index as u64 + 1;
            PlayerState {
                player,
                capture: Some(capture(0, None, player.0 as u64)),
                competition: (index % 2 == 0).then(|| selected(player)),
                completion: None,
                score: ScoreSummary {
                    hits,
                    combo: hits,
                    max_combo: hits,
                    grades: [(u32::MAX, hits)].into_iter().collect(),
                    ..Default::default()
                },
                gauge: BmsGauge::default(),
                last_song: Timestamp::ZERO,
            }
        })
        .collect()
}
fn completed() -> CompletedPlayResult {
    CompletedPlayResult::from_completed(Timestamp::ZERO, None, &BmsGauge::default())
}
fn paths(states: &[PlayerState]) -> Vec<(PlayerId, Option<PathBuf>)> {
    states
        .iter()
        .rev()
        .map(|state| {
            (
                state.player,
                Some(PathBuf::from(format!("member{}.bkr", state.player.0))),
            )
        })
        .collect()
}
#[test]
fn actual_common_cohort_finish_retains_one_two_sixty_four_original_ids_scores_and_selected_none_rows()
 {
    for count in [1u32, 2, 64] {
        let states = states(count);
        let ids: Vec<_> = states.iter().map(|state| state.player).collect();
        let destinations = paths(&states);
        let outcomes: Vec<_> = ids.iter().rev().map(|id| (*id, completed())).collect();
        let calls = RefCell::new(Vec::new());
        finish_cohort_with_results_and_network(
            Ok(Some(outcomes.clone())),
            states,
            None,
            destinations,
            vec![],
            Some(Path::new("whole.bkr")),
            |capture, path, failed| {
                assert!(!failed);
                let capture = capture.unwrap();
                let id = PlayerId(
                    u32::try_from(
                        crate::replay_playback::decode_section_setup(&capture.header().options)
                            .unwrap()
                            .chart_seed,
                    )
                    .unwrap(),
                );
                assert_eq!(path, Some(Path::new(&format!("member{}.bkr", id.0))));
                calls.borrow_mut().push(id);
                Ok(())
            },
            |archive, base| {
                assert_eq!(*calls.borrow(), ids);
                assert_eq!(base, Some(Path::new("whole.bkr")));
                let decoded = decode_archive(&encode_archive(archive).unwrap()).unwrap();
                assert_eq!(
                    decoded
                        .entries()
                        .iter()
                        .map(|entry| entry.player)
                        .collect::<Vec<_>>(),
                    outcomes.iter().map(|row| row.0).collect::<Vec<_>>()
                );
                for entry in decoded.entries() {
                    let index = (u32::MAX - entry.player.0) / 17;
                    let hits = index as u64 + 1;
                    assert_eq!(
                        entry.score,
                        Some(
                            ArchivedScore::from_summary(&ScoreSummary {
                                hits,
                                combo: hits,
                                max_combo: hits,
                                grades: [(u32::MAX, hits)].into_iter().collect(),
                                ..Default::default()
                            })
                            .unwrap()
                        )
                    );
                    assert_eq!(decoded.comparison(entry.player).is_some(), index % 2 == 0);
                    if let Some(snapshot) = decoded.comparison(entry.player) {
                        assert_eq!(
                            snapshot.ghosts[0].label,
                            format!("member{}.bkr", entry.player.0)
                        );
                        assert!(snapshot.network.is_none());
                    }
                    let member = decoded.for_player(entry.player).unwrap();
                    assert_eq!(
                        member.comparison(entry.player),
                        decoded.comparison(entry.player)
                    );
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*calls.borrow(), ids);
    }
}
#[test]
fn later_archive_refusal_and_replay_failures_attempt_every_member_and_retain_original_typed_box_first()
 {
    for mask in 0..16 {
        let mut states = states(3);
        if mask & 2 != 0 {
            states[2].score.hits = 0;
        }
        // Fault setup uses valid recordings beyond the archive projection's
        // eight-opponent bound, without fabricating network failure evidence.
        if mask & 8 != 0 {
            states[2].competition = Some(selected_count(states[2].player, 9));
        }
        let ids: Vec<_> = states.iter().map(|state| state.player).collect();
        let destinations = paths(&states);
        let proof: Vec<_> = ids.iter().map(|id| (*id, completed())).collect();
        let boxed = Box::new(CompletedLocalPublicationError {
            results: proof.clone(),
            cause: Box::new(Fault(Arc::new(91))),
        });
        let pointer = boxed.as_ref() as *const CompletedLocalPublicationError;
        let outcome = if mask & 1 != 0 {
            Err(boxed as Box<dyn Error>)
        } else {
            Ok(Some(proof.clone()))
        };
        let calls = RefCell::new(Vec::new());
        let archives = RefCell::new(0);
        let result = finish_cohort_with_results_and_network(
            outcome,
            states,
            None,
            destinations,
            vec![],
            Some(Path::new("whole.bkr")),
            |capture, _, failed| {
                assert_eq!(failed, mask & 1 != 0);
                let id = PlayerId(
                    u32::try_from(
                        crate::replay_playback::decode_section_setup(
                            &capture.unwrap().header().options,
                        )
                        .unwrap()
                        .chart_seed,
                    )
                    .unwrap(),
                );
                calls.borrow_mut().push(id);
                if mask & 4 != 0 && (id == ids[0] || id == ids[2]) {
                    Err(Box::new(Fault(Arc::new(id.0 as usize))))
                } else {
                    Ok(())
                }
            },
            |archive, _| {
                assert_eq!(*calls.borrow(), ids);
                assert!(archive.comparisons().is_some());
                *archives.borrow_mut() += 1;
                Ok(())
            },
        );
        assert_eq!(*calls.borrow(), ids);
        assert_eq!(*archives.borrow(), usize::from(mask & 10 == 0));
        if mask == 0 {
            result.unwrap();
        } else if mask & 1 != 0 {
            let error = result.unwrap_err();
            let retained = error
                .downcast_ref::<CompletedLocalPublicationError>()
                .unwrap();
            assert_eq!(retained as *const _, pointer);
            assert_eq!(retained.results, proof);
        } else if mask & 4 != 0 {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("cohort replay refusal")
            );
        } else if mask & 2 != 0 {
            assert!(
                result
                    .unwrap_err()
                    .downcast_ref::<crate::result_archive::ArchiveError>()
                    .is_some()
            );
        } else {
            assert!(result.unwrap_err().to_string().contains("opponent count"));
        }
    }
}
#[test]
fn no_recording_base_skips_new_archive_work_and_no_selected_owner_keeps_legacy_score_only() {
    let mut states = states(2);
    states[1].score.hits = 0;
    let destinations = paths(&states);
    let calls = RefCell::new(0);
    let proof: Vec<_> = states
        .iter()
        .map(|state| (state.player, completed()))
        .collect();
    finish_cohort_with_results_and_network(
        Ok(Some(proof)),
        states,
        None,
        destinations,
        vec![],
        None,
        |_, _, _| {
            *calls.borrow_mut() += 1;
            Ok(())
        },
        |_, _| panic!("no base path may publish archive"),
    )
    .unwrap();
    assert_eq!(*calls.borrow(), 2);
    let mut states = self::states(2);
    for state in &mut states {
        state.competition = None;
    }
    let destinations = paths(&states);
    let proof: Vec<_> = states
        .iter()
        .map(|state| (state.player, completed()))
        .collect();
    finish_cohort_with_results_and_network(
        Ok(Some(proof)),
        states,
        None,
        destinations,
        vec![],
        Some(Path::new("whole.bkr")),
        |_, _, _| Ok(()),
        |archive, _| {
            assert!(archive.comparisons().is_none());
            assert_eq!(
                &encode_archive(archive).unwrap()[8..12],
                &2u32.to_le_bytes()
            );
            Ok(())
        },
    )
    .unwrap();
}
