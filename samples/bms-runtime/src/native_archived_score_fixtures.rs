//! Deferred scored native-save policy, with injected historical numeric states.
use crate::{
    native_completed_save::*,
    native_gameplay::NativeGameplayResult,
    competition::ScoreSummary,
    gauge::BmsGauge,
    local_players::PlayerId,
    native_section_capture_fixtures::capture,
    native_finish::finish_solo_with_result_and_score,
    play_result::{
        CompletedPlayResult, CompletedSoloPublicationError, CompletedLocalPublicationError,
    },
    result_archive::{encode_archive, decode_archive, ArchivedScore},
};
use beatkernel::time::Timestamp;
use std::{error::Error, fmt, path::Path, sync::Arc, cell::RefCell};
#[derive(Debug)]
struct Fault(Arc<usize>);
impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("injected save refusal")
    }
}
impl Error for Fault {}
fn fault(token: &Arc<usize>) -> Box<dyn Error> {
    Box::new(Fault(token.clone()))
}
fn stage(token: &Arc<usize>, fails: bool) -> NativeGameplayResult<()> {
    if fails { Err(fault(token)) } else { Ok(()) }
}
fn score(hits: u64, grade: u32) -> ScoreSummary {
    ScoreSummary {
        hits,
        combo: hits,
        max_combo: hits,
        grades: [(grade, hits)].into_iter().collect(),
        ..Default::default()
    }
}
fn completed(start: i64, end: Option<i64>) -> CompletedPlayResult {
    CompletedPlayResult::from_completed(
        Timestamp::from_nanos(start),
        end.map(Timestamp::from_nanos),
        &BmsGauge::default(),
    )
}
#[test]
fn scored_solo_preserves_full_or_practice_capture_and_publication_failure_proof_while_legacy_is_v1()
{
    for (start, end) in [(0, None), (72_000_000_000_000, Some(604_800_000_000_000))] {
        let capture = capture(start, end, u64::MAX);
        let profile = BmsGauge::default().profile().clone();
        let result = completed(start, end);
        let score = score(u64::MAX, u32::MAX);
        let healthy = Ok(Some(result));
        let scored = solo_archive_with_score(&healthy, Some(&capture), &profile, &score)
            .unwrap()
            .unwrap();
        assert_eq!(scored.entries()[0].header, *capture.header());
        assert_eq!(scored.entries()[0].result.gauge, result.gauge());
        assert_eq!(
            scored.entries()[0].score,
            Some(ArchivedScore::from_summary(&score).unwrap())
        );
        let bytes = encode_archive(&scored).unwrap();
        assert_eq!(&bytes[8..12], &2u32.to_le_bytes());
        let token = Arc::new(1);
        let failed: NativeGameplayResult<Option<CompletedPlayResult>> =
            Err(Box::new(CompletedSoloPublicationError {
                result,
                cause: fault(&token),
            }));
        assert_eq!(
            solo_archive_with_score(&failed, Some(&capture), &profile, &score)
                .unwrap()
                .unwrap(),
            scored
        );
        assert!(Arc::ptr_eq(
            &failed
                .as_ref()
                .unwrap_err()
                .downcast_ref::<CompletedSoloPublicationError>()
                .unwrap()
                .cause
                .downcast_ref::<Fault>()
                .unwrap()
                .0,
            &token
        ));
        let legacy = solo_archive(&healthy, Some(&capture), &profile)
            .unwrap()
            .unwrap();
        assert!(legacy.entries()[0].score.is_none());
        assert_eq!(
            &encode_archive(&legacy).unwrap()[8..12],
            &1u32.to_le_bytes()
        );
    }
}
#[test]
fn recorded_prefix_disabled_capture_and_untyped_error_never_promote_score_into_completion() {
    let capture = capture(0, None, 7);
    let profile = BmsGauge::default().profile().clone();
    let mut invalid_score = score(1, 7);
    invalid_score.hits = 0;
    for outcome in [Ok(None), Err(fault(&Arc::new(9)))] {
        assert!(
            solo_archive_with_score(&outcome, Some(&capture), &profile, &invalid_score)
                .unwrap()
                .is_none()
        );
    }
    let healthy = Ok(Some(completed(0, None)));
    assert!(
        solo_archive_with_score(&healthy, None, &profile, &invalid_score)
            .unwrap()
            .is_none()
    );
    assert!(solo_archive_with_score(&healthy, Some(&capture), &profile, &invalid_score).is_err());
}
#[test]
fn scored_cohort_associates_all_original_ids_and_shuffled_score_rows_exactly() {
    for count in [1u32, 2, 64] {
        let ids: Vec<_> = (0..count)
            .map(|index| PlayerId(u32::MAX - index * 17))
            .collect();
        let captures: Vec<_> = ids.iter().map(|id| capture(0, None, id.0 as u64)).collect();
        let profile = BmsGauge::default().profile().clone();
        let members: Vec<_> = ids
            .iter()
            .zip(&captures)
            .rev()
            .map(|(id, capture)| ArchiveMember {
                player: *id,
                capture: Some(capture),
                profile: &profile,
            })
            .collect();
        let summaries: Vec<_> = ids
            .iter()
            .enumerate()
            .map(|(index, _)| score(index as u64 + 1, u32::MAX - index as u32))
            .collect();
        let scores: Vec<_> = ids
            .iter()
            .zip(&summaries)
            .rev()
            .map(|(id, score)| (*id, score))
            .collect();
        let result = completed(0, None);
        let outcome = Ok(Some(ids.iter().map(|id| (*id, result)).collect()));
        assert!(
            cohort_archive_with_scores(&Ok(None), &members, &scores)
                .unwrap()
                .is_none()
        );
        let archive = cohort_archive_with_scores(&outcome, &members, &scores)
            .unwrap()
            .unwrap();
        let decoded = decode_archive(&encode_archive(&archive).unwrap()).unwrap();
        for (index, row) in decoded.entries().iter().enumerate() {
            assert_eq!(row.player, ids[index]);
            assert_eq!(row.header, *captures[index].header());
            assert_eq!(
                row.score,
                Some(ArchivedScore::from_summary(&summaries[index]).unwrap())
            );
            assert_eq!(row.result.gauge, result.gauge());
        }
    }
}
#[test]
fn whole_score_roster_refusal_is_atomic_and_keeps_original_local_publication_error() {
    let ids = [PlayerId(u32::MAX), PlayerId(7)];
    let captures = [capture(0, None, 1), capture(0, None, 2)];
    let profile = BmsGauge::default().profile().clone();
    let members: Vec<_> = ids
        .iter()
        .zip(&captures)
        .map(|(id, capture)| ArchiveMember {
            player: *id,
            capture: Some(capture),
            profile: &profile,
        })
        .collect();
    let result = completed(0, None);
    let token = Arc::new(71);
    let outcome: NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> =
        Err(Box::new(CompletedLocalPublicationError {
            results: ids.iter().map(|id| (*id, result)).collect(),
            cause: fault(&token),
        }));
    let valid = score(1, 0);
    let mut invalid = valid.clone();
    invalid.combo = 2;
    for scores in [
        vec![],
        vec![(ids[0], &valid)],
        vec![(ids[0], &valid), (ids[0], &valid)],
        vec![(ids[0], &valid), (PlayerId(9), &valid)],
        vec![(ids[0], &valid), (ids[1], &invalid)],
    ] {
        assert!(cohort_archive_with_scores(&outcome, &members, &scores).is_err());
    }
    let archive =
        cohort_archive_with_scores(&outcome, &members, &[(ids[1], &valid), (ids[0], &valid)])
            .unwrap()
            .unwrap();
    assert_eq!(
        archive
            .entries()
            .iter()
            .map(|row| row.player)
            .collect::<Vec<_>>(),
        ids
    );
    let retained = outcome
        .as_ref()
        .unwrap_err()
        .downcast_ref::<CompletedLocalPublicationError>()
        .unwrap();
    assert_eq!(retained.results, ids.map(|id| (id, result)));
    assert!(Arc::ptr_eq(
        &retained.cause.downcast_ref::<Fault>().unwrap().0,
        &token
    ));
    let missing_capture = [
        ArchiveMember {
            player: ids[0],
            capture: Some(&captures[0]),
            profile: &profile,
        },
        ArchiveMember {
            player: ids[1],
            capture: None,
            profile: &profile,
        },
    ];
    assert!(
        cohort_archive_with_scores(
            &outcome,
            &missing_capture,
            &[(ids[0], &valid), (ids[1], &valid)]
        )
        .is_err()
    );
}
#[test]
fn scored_finish_attempts_replay_before_archive_for_all_first_error_combinations() {
    let tokens: [Arc<usize>; 5] = std::array::from_fn(Arc::new);
    for mask in 0..32 {
        let result = completed(0, None);
        let boxed = Box::new(CompletedSoloPublicationError {
            result,
            cause: fault(&tokens[0]),
        });
        let pointer = boxed.as_ref() as *const CompletedSoloPublicationError;
        let outcome = if mask & 1 != 0 {
            Err(boxed as Box<dyn Error>)
        } else {
            Ok(Some(result))
        };
        let summary = score(9_007_199_254_740_993, u32::MAX);
        let profile = BmsGauge::default().profile().clone();
        let events = RefCell::new(Vec::new());
        let returned = finish_solo_with_result_and_score(
            outcome,
            stage(&tokens[1], mask & 2 != 0),
            stage(&tokens[2], mask & 4 != 0),
            None,
            Some(capture(0, None, u64::MAX)),
            &profile,
            &summary,
            Some(Path::new("memory.bkr")),
            |capture, path, failed| {
                events.borrow_mut().push("replay");
                assert!(capture.is_some());
                assert_eq!(path, Some(Path::new("memory.bkr")));
                assert_eq!(failed, mask & 7 != 0);
                stage(&tokens[3], mask & 8 != 0)
            },
            |archive, path| {
                events.borrow_mut().push("archive");
                assert_eq!(path, Some(Path::new("memory.bkr")));
                assert_eq!(
                    archive.entries()[0].score,
                    Some(ArchivedScore::from_summary(&summary).unwrap())
                );
                stage(&tokens[4], mask & 16 != 0)
            },
        );
        assert_eq!(*events.borrow(), ["replay", "archive"]);
        if mask == 0 {
            returned.unwrap();
        } else {
            let error = returned.unwrap_err();
            let first = (0..5).find(|index| mask & (1 << index) != 0).unwrap();
            if first == 0 {
                let original = error
                    .downcast_ref::<CompletedSoloPublicationError>()
                    .unwrap();
                assert_eq!(original as *const _, pointer);
                assert_eq!(original.result, result);
            } else {
                assert!(Arc::ptr_eq(
                    &error.downcast_ref::<Fault>().unwrap().0,
                    &tokens[first]
                ));
            }
        }
    }
}

#[test]
fn scored_validation_refusal_still_saves_replay_and_never_publishes_partial_archive() {
    let mut invalid = score(1, 7);
    invalid.hits = 0;
    let profile = BmsGauge::default().profile().clone();
    let calls = RefCell::new(Vec::new());
    let result = finish_solo_with_result_and_score(
        Ok(Some(completed(0, None))),
        Ok(()),
        Ok(()),
        None,
        Some(capture(0, None, 7)),
        &profile,
        &invalid,
        Some(Path::new("memory.bkr")),
        |capture, _, failed| {
            calls.borrow_mut().push("replay");
            assert!(capture.is_some());
            assert!(!failed);
            Ok(())
        },
        |_, _| {
            calls.borrow_mut().push("archive");
            Ok(())
        },
    );
    assert!(
        result
            .unwrap_err()
            .downcast_ref::<crate::result_archive::ArchiveError>()
            .is_some()
    );
    assert_eq!(*calls.borrow(), ["replay"]);
}
