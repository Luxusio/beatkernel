//! Deferred actual shared solo finalization; selected comparison owns no endpoint.
use super::*;
use crate::{
    competition::{Competition, OpponentKind, ScoreSummary},
    native_section_capture_fixtures::capture,
    native_solo_result_fixtures::result_source,
    play_result::{CompletedPlayResult, CompletedSoloPublicationError},
    gauge::BmsGauge,
    result_archive::{ArchivedScore, encode_archive, decode_archive},
};
use beatkernel::{input::CodecLimits, replay::codec::ReplayCodecLimits, time::Timestamp};
use std::{cell::RefCell, sync::Arc, error::Error, fmt};
#[derive(Debug)]
struct Fault(Arc<usize>);
impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("injected native finish refusal")
    }
}
impl Error for Fault {}
fn fault(token: &Arc<usize>) -> Box<dyn Error> {
    Box::new(Fault(token.clone()))
}
fn stage(token: &Arc<usize>, fails: bool) -> NativeGameplayResult<()> {
    if fails { Err(fault(token)) } else { Ok(()) }
}
fn selected() -> LiveCompetition {
    let file = capture(0, None, 0).into_file();
    let mut competition = Competition::new(file.header.clone(), 1).unwrap();
    let limits =
        ReplayCodecLimits::new(65536, 32, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    competition
        .add_replay(
            &result_source(false),
            file,
            limits,
            OpponentKind::Own,
            format!("{}/raw\nname.bkr", "directory".repeat(40)),
        )
        .unwrap();
    LiveCompetition::from_prepared(
        crate::local_players::PlayerId(1),
        competition,
        None,
        std::time::Duration::from_secs(1),
    )
    .unwrap()
}
fn completed() -> CompletedPlayResult {
    CompletedPlayResult::from_completed(Timestamp::ZERO, None, &BmsGauge::default())
}
fn score() -> ScoreSummary {
    ScoreSummary {
        hits: 3,
        combo: 3,
        max_combo: 3,
        grades: [(u32::MAX, 3)].into_iter().collect(),
        ..Default::default()
    }
}
#[test]
fn actual_offline_selected_owner_archives_bounded_retained_prefix_after_finish_and_legacy_none_stays_v2()
 {
    for selected_owner in [false, true] {
        let mut competition = selected_owner.then(selected);
        let gauge = BmsGauge::default();
        let score = score();
        let calls = RefCell::new(Vec::new());
        finish_solo_with_result_and_score(
            Ok(Some(completed())),
            Ok(()),
            Ok(()),
            competition.as_mut(),
            Some(capture(0, None, 0)),
            gauge.profile(),
            &score,
            Some(Path::new("original.bkr")),
            |capture, _, failed| {
                assert!(capture.is_some());
                assert!(!failed);
                calls.borrow_mut().push("replay");
                Ok(())
            },
            |archive, _| {
                calls.borrow_mut().push("archive");
                assert_eq!(
                    archive.entries()[0].score,
                    Some(ArchivedScore::from_summary(&score).unwrap())
                );
                let bytes = encode_archive(archive).unwrap();
                let decoded = decode_archive(&bytes).unwrap();
                if selected_owner {
                    let snapshot = decoded
                        .comparison(crate::local_players::PlayerId(1))
                        .unwrap();
                    assert_eq!(snapshot.ghosts[0].label, "rawname.bkr");
                    assert_eq!(
                        (
                            snapshot.ghosts[0].hits,
                            snapshot.ghosts[0].misses,
                            snapshot.ghosts[0].recorded_until
                        ),
                        (0, 0, None)
                    );
                    assert!(snapshot.network.is_none());
                    assert_eq!(&bytes[8..12], &3u32.to_le_bytes());
                } else {
                    assert!(decoded.comparisons().is_none());
                    assert_eq!(&bytes[8..12], &2u32.to_le_bytes());
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*calls.borrow(), ["replay", "archive"]);
        if let Some(owner) = &competition {
            assert_eq!(
                owner.archive_snapshot().unwrap().ghosts[0].label,
                "rawname.bkr"
            );
        }
    }
}
#[test]
fn all_first_error_combinations_keep_original_publication_box_and_attempt_both_saves_in_order() {
    let tokens: [Arc<usize>; 5] = std::array::from_fn(Arc::new);
    for mask in 0..32 {
        let mut owner = selected();
        let gauge = BmsGauge::default();
        let score = score();
        let calls = RefCell::new(Vec::new());
        let boxed = Box::new(CompletedSoloPublicationError {
            result: completed(),
            cause: fault(&tokens[0]),
        });
        let pointer = boxed.as_ref() as *const CompletedSoloPublicationError;
        let outcome = if mask & 1 != 0 {
            Err(boxed as Box<dyn Error>)
        } else {
            Ok(Some(completed()))
        };
        let result = finish_solo_with_result_and_score(
            outcome,
            stage(&tokens[1], mask & 2 != 0),
            stage(&tokens[2], mask & 4 != 0),
            Some(&mut owner),
            Some(capture(0, None, 0)),
            gauge.profile(),
            &score,
            Some(Path::new("original.bkr")),
            |_, _, failed| {
                calls.borrow_mut().push("replay");
                assert_eq!(failed, mask & 7 != 0);
                stage(&tokens[3], mask & 8 != 0)
            },
            |archive, _| {
                calls.borrow_mut().push("archive");
                assert!(archive.comparisons().is_some());
                stage(&tokens[4], mask & 16 != 0)
            },
        );
        assert_eq!(*calls.borrow(), ["replay", "archive"]);
        if mask == 0 {
            result.unwrap();
        } else {
            let error = result.unwrap_err();
            let first = (0..5).find(|index| mask & (1 << index) != 0).unwrap();
            if first == 0 {
                let original = error
                    .downcast_ref::<CompletedSoloPublicationError>()
                    .unwrap();
                assert_eq!(original as *const _, pointer);
                assert_eq!(original.result, completed());
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
fn prefix_no_completion_and_no_base_path_skip_new_archival_validation_but_still_save_capture() {
    for (proof, base) in [(false, true), (true, false)] {
        let mut owner = selected();
        let gauge = BmsGauge::default();
        let mut invalid_score = score();
        invalid_score.hits = 0;
        let calls = RefCell::new(Vec::new());
        finish_solo_with_result_and_score(
            Ok(proof.then(completed)),
            Ok(()),
            Ok(()),
            Some(&mut owner),
            Some(capture(0, None, 0)),
            gauge.profile(),
            &invalid_score,
            base.then_some(Path::new("original.bkr")),
            |capture, _, _| {
                assert!(capture.is_some());
                calls.borrow_mut().push("replay");
                Ok(())
            },
            |_, _| {
                calls.borrow_mut().push("archive");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*calls.borrow(), ["replay"]);
    }
}
