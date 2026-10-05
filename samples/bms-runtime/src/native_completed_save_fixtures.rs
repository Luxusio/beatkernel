//! Deferred shared finalization with original error identity and scripted effects.
use crate::{
    native_completed_save::*,
    native_gameplay::NativeGameplayResult,
    native_cohort::PlayerState,
    local_players::PlayerId,
    gauge::BmsGauge,
    play_result::{
        CompletedPlayResult, CompletedSoloPublicationError, CompletedLocalPublicationError,
    },
    native_section_capture_fixtures::capture,
};
use beatkernel::time::Timestamp;
use std::{
    cell::RefCell,
    error::Error,
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};
#[derive(Debug)]
struct Marker(Arc<usize>);
impl fmt::Display for Marker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "marker{}", self.0)
    }
}
impl Error for Marker {}
fn error(token: &Arc<usize>) -> Box<dyn Error> {
    Box::new(Marker(token.clone()))
}
fn stage(token: &Arc<usize>, fails: bool) -> NativeGameplayResult<()> {
    if fails { Err(error(token)) } else { Ok(()) }
}
fn completed() -> CompletedPlayResult {
    CompletedPlayResult::from_completed(Timestamp::ZERO, None, &BmsGauge::default())
}
fn state(player: PlayerId, seed: u64) -> PlayerState {
    PlayerState {
        player,
        capture: Some(capture(0, None, seed)),
        competition: None,
        completion: None,
        score: Default::default(),
        gauge: BmsGauge::default(),
        last_song: Timestamp::ZERO,
    }
}
fn archive_members(states: &[PlayerState]) -> Vec<ArchiveMember<'_>> {
    states
        .iter()
        .map(|state| ArchiveMember {
            player: state.player,
            capture: state.capture.as_ref(),
            profile: state.gauge.profile(),
        })
        .collect()
}
#[test]
fn solo_typed_completion_and_publication_failure_preserve_proof_without_prefix_promotion() {
    let recording = capture(0, None, 0);
    let profile = BmsGauge::default().profile().clone();
    let result = completed();
    let healthy: NativeGameplayResult<Option<CompletedPlayResult>> = Ok(Some(result));
    let archive = solo_archive(&healthy, Some(&recording), &profile)
        .unwrap()
        .unwrap();
    assert_eq!(archive.entries()[0].result.gauge, result.gauge());
    assert_eq!(archive.entries()[0].header, *recording.header());
    let token = Arc::new(91);
    let failed: NativeGameplayResult<Option<CompletedPlayResult>> =
        Err(Box::new(CompletedSoloPublicationError {
            result,
            cause: error(&token),
        }));
    assert_eq!(
        solo_archive(&failed, Some(&recording), &profile)
            .unwrap()
            .unwrap(),
        archive
    );
    assert!(Arc::ptr_eq(
        &failed
            .as_ref()
            .unwrap_err()
            .downcast_ref::<CompletedSoloPublicationError>()
            .unwrap()
            .cause
            .downcast_ref::<Marker>()
            .unwrap()
            .0,
        &token
    ));
    assert!(solo_archive(&healthy, None, &profile).unwrap().is_none());
    for outcome in [Ok(None), Err(error(&token))] {
        assert!(
            solo_archive(&outcome, Some(&recording), &profile)
                .unwrap()
                .is_none()
        );
    }
}
#[test]
fn every_first_error_combination_attempts_replay_then_archive_without_erasing_proof() {
    let tokens: [Arc<usize>; 4] = std::array::from_fn(Arc::new);
    for mask in 0..16 {
        let recording = capture(0, None, 0);
        let profile = BmsGauge::default().profile().clone();
        let completed = completed();
        let outcome: NativeGameplayResult<Option<CompletedPlayResult>> = if mask & 1 != 0 {
            Err(Box::new(CompletedSoloPublicationError {
                result: completed,
                cause: error(&tokens[0]),
            }))
        } else {
            Ok(Some(completed))
        };
        let archive = solo_archive(&outcome, Some(&recording), &profile);
        let calls = RefCell::new(Vec::new());
        let result = finalize_completed_save(
            outcome,
            stage(&tokens[1], mask & 2 != 0),
            archive,
            || {
                calls.borrow_mut().push("replay");
                stage(&tokens[2], mask & 4 != 0)
            },
            |archive| {
                calls.borrow_mut().push("archive");
                assert_eq!(archive.entries()[0].result.gauge, completed.gauge());
                stage(&tokens[3], mask & 8 != 0)
            },
        );
        assert_eq!(*calls.borrow(), ["replay", "archive"]);
        if mask == 0 {
            assert!(result.is_ok());
        } else {
            let first = (0..4).find(|index| mask & (1 << index) != 0).unwrap();
            let returned = result.unwrap_err();
            if first == 0 {
                let original = returned
                    .downcast_ref::<CompletedSoloPublicationError>()
                    .unwrap();
                assert_eq!(original.result, completed);
                assert!(Arc::ptr_eq(
                    &original.cause.downcast_ref::<Marker>().unwrap().0,
                    &tokens[0]
                ));
            } else {
                assert!(Arc::ptr_eq(
                    &returned.downcast_ref::<Marker>().unwrap().0,
                    &tokens[first]
                ));
            }
        }
    }
}
#[test]
fn cancelled_disabled_prefix_and_late_validation_failure_never_publish_an_archive() {
    let token = Arc::new(17);
    let recording = capture(0, None, 0);
    let profile = BmsGauge::default().profile().clone();
    for outcome in [Ok(None), Err(error(&token))] {
        let archive = solo_archive(&outcome, Some(&recording), &profile);
        let calls = RefCell::new(Vec::new());
        let _ = finalize_completed_save(
            outcome,
            Ok(()),
            archive,
            || {
                calls.borrow_mut().push("prefix");
                Ok(())
            },
            |_| {
                calls.borrow_mut().push("archive");
                Ok(())
            },
        );
        assert_eq!(*calls.borrow(), ["prefix"]);
    }
    let outcome: NativeGameplayResult<Option<CompletedPlayResult>> = Ok(Some(completed()));
    let calls = RefCell::new(Vec::new());
    finalize_completed_save(
        outcome,
        Ok(()),
        Ok(None),
        || {
            calls.borrow_mut().push("disabled");
            Ok(())
        },
        |_| {
            calls.borrow_mut().push("archive");
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(*calls.borrow(), ["disabled"]);
    let mut states = vec![state(PlayerId(99), 3), state(PlayerId(u32::MAX), 7)];
    states[1].capture = Some(capture(20_000_000, None, 7)); // Bad later member's pristine header extent.
    let outcome = Ok(Some(vec![
        (PlayerId(99), completed()),
        (PlayerId(u32::MAX), completed()),
    ]));
    let archive = cohort_archive(&outcome, &archive_members(&states));
    assert!(archive.is_err());
    let calls = RefCell::new(Vec::new());
    assert!(
        finalize_completed_save(
            outcome,
            Ok(()),
            archive,
            || {
                calls.borrow_mut().push("replays");
                Ok(())
            },
            |_| {
                calls.borrow_mut().push("archive");
                Ok(())
            }
        )
        .is_err()
    );
    assert_eq!(*calls.borrow(), ["replays"]);
    for bad in 0..3 {
        let mut states = vec![state(PlayerId(99), 3), state(PlayerId(u32::MAX), 7)];
        match bad {
            0 => states[1].player = PlayerId(99),
            1 => {
                states[1].gauge = BmsGauge::new(
                    crate::gauge::GaugeProfile::new(20_000_000, 0, 0, 0, false, vec![]).unwrap(),
                )
            }
            _ => states[1].capture = None,
        }
        let outcome = Ok(Some(vec![
            (PlayerId(99), completed()),
            (PlayerId(u32::MAX), completed()),
        ]));
        assert!(
            cohort_archive(&outcome, &archive_members(&states)).is_err(),
            "bad later member {bad} accepted"
        );
    }
    let mut disabled = vec![state(PlayerId(99), 3), state(PlayerId(u32::MAX), 7)];
    for state in &mut disabled {
        state.capture = None;
    }
    let outcome = Ok(Some(vec![
        (PlayerId(99), completed()),
        (PlayerId(u32::MAX), completed()),
    ]));
    assert!(
        cohort_archive(&outcome, &archive_members(&disabled))
            .unwrap()
            .is_none()
    );
}
#[test]
fn all_roster_sizes_preserve_ids_headers_profiles_and_typed_local_error() {
    for count in 1..=64 {
        let states = (0..count)
            .map(|index| state(PlayerId(u32::MAX - index * 17), index as u64))
            .collect::<Vec<_>>();
        let rows = states
            .iter()
            .rev()
            .map(|state| (state.player, completed()))
            .collect::<Vec<_>>();
        let cause = Arc::new(81);
        let outcome: NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> =
            Err(Box::new(CompletedLocalPublicationError {
                results: rows.clone(),
                cause: error(&cause),
            }));
        let archive = cohort_archive(&outcome, &archive_members(&states))
            .unwrap()
            .unwrap();
        assert_eq!(
            archive
                .entries()
                .iter()
                .map(|entry| entry.player)
                .collect::<Vec<_>>(),
            rows.iter().map(|row| row.0).collect::<Vec<_>>()
        );
        for entry in archive.entries() {
            let state = states
                .iter()
                .find(|state| state.player == entry.player)
                .unwrap();
            assert_eq!(entry.header, *state.capture.as_ref().unwrap().header());
            assert_eq!(entry.profile, *state.gauge.profile());
        }
        let returned =
            finalize_completed_save(outcome, Ok(()), Ok(Some(archive)), || Ok(()), |_| Ok(()))
                .unwrap_err();
        assert!(Arc::ptr_eq(
            &returned
                .downcast_ref::<CompletedLocalPublicationError>()
                .unwrap()
                .cause
                .downcast_ref::<Marker>()
                .unwrap()
                .0,
            &cause
        ));
    }
}
#[test]
fn actual_cohort_finalizer_attempts_each_replay_by_id_before_one_base_path_archive() {
    let ids = [PlayerId(99), PlayerId(7), PlayerId(u32::MAX)];
    let states = ids
        .into_iter()
        .enumerate()
        .map(|(index, id)| state(id, index as u64 + 10))
        .collect::<Vec<_>>();
    let paths = vec![
        (ids[2], Some(PathBuf::from("max.bkr"))),
        (ids[0], Some(PathBuf::from("first.bkr"))),
        (ids[1], Some(PathBuf::from("middle.bkr"))),
    ];
    let cause = Arc::new(92);
    let result = completed();
    let outcome: NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> =
        Err(Box::new(CompletedLocalPublicationError {
            results: ids.into_iter().rev().map(|id| (id, result)).collect(),
            cause: error(&cause),
        }));
    let calls = RefCell::new(Vec::new());
    let archive_error = Arc::new(93);
    let returned = crate::native_cohort_setup::finish_cohort_with_results(
        outcome,
        states,
        paths,
        vec!["input cleanup refused".into()],
        Some(Path::new("entire-base.bkr")),
        |capture, path, failed| {
            assert!(failed);
            let capture = capture.unwrap();
            let seed = crate::replay_playback::decode_section_setup(&capture.header().options)
                .unwrap()
                .chart_seed;
            let expected = match seed {
                10 => "first.bkr",
                11 => "middle.bkr",
                12 => "max.bkr",
                _ => panic!("unknown seed"),
            };
            assert_eq!(path, Some(Path::new(expected)));
            calls.borrow_mut().push(expected.to_owned());
            Err("replay refused".into())
        },
        |archive, base| {
            assert_eq!(base, Some(Path::new("entire-base.bkr")));
            assert_eq!(
                archive
                    .entries()
                    .iter()
                    .map(|row| row.player)
                    .collect::<Vec<_>>(),
                [ids[2], ids[1], ids[0]]
            );
            assert_eq!(*calls.borrow(), ["first.bkr", "middle.bkr", "max.bkr"]);
            calls.borrow_mut().push("archive".into());
            Err(error(&archive_error))
        },
    )
    .unwrap_err();
    assert_eq!(
        *calls.borrow(),
        ["first.bkr", "middle.bkr", "max.bkr", "archive"]
    );
    assert!(Arc::ptr_eq(
        &returned
            .downcast_ref::<CompletedLocalPublicationError>()
            .unwrap()
            .cause
            .downcast_ref::<Marker>()
            .unwrap()
            .0,
        &cause
    ));
}
#[test]
fn sidecar_derivation_appends_entire_unicode_native_filename_without_io() {
    let base = PathBuf::from("recordings").join("노래.🎵.take.bkr");
    let derived = crate::native_result_archive::sidecar_path(&base).unwrap();
    assert_eq!(
        derived,
        PathBuf::from("recordings").join("노래.🎵.take.bkr.bkresult")
    );
    assert!(crate::native_result_archive::sidecar_path(Path::new("/")).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStringExt, OsStrExt};
        let base = PathBuf::from(std::ffi::OsString::from_vec(vec![
            b'n', 0xff, b'.', b'b', b'k', b'r',
        ]));
        let derived = crate::native_result_archive::sidecar_path(&base).unwrap();
        assert_eq!(
            derived.as_os_str().as_bytes(),
            &[
                b'n', 0xff, b'.', b'b', b'k', b'r', b'.', b'b', b'k', b'r', b'e', b's', b'u', b'l',
                b't'
            ]
        );
    }
}
#[test]
fn actual_solo_finalizer_keeps_cleanup_error_and_completion_while_attempting_both_saves() {
    let output_error = Arc::new(121);
    let input_error = Arc::new(122);
    let archive_error = Arc::new(123);
    let profile = BmsGauge::default().profile().clone();
    let result = completed();
    let calls = RefCell::new(Vec::new());
    let returned = crate::native_finish::finish_solo_with_result(
        Ok(Some(result)),
        Err(error(&output_error)),
        Err(error(&input_error)),
        None,
        Some(capture(0, None, 0)),
        &profile,
        Some(Path::new("solo.bkr")),
        |recording, path, failed| {
            assert!(recording.is_some());
            assert_eq!(path, Some(Path::new("solo.bkr")));
            assert!(failed);
            calls.borrow_mut().push("replay");
            Err("replay refused".into())
        },
        |archive, path| {
            assert_eq!(path, Some(Path::new("solo.bkr")));
            assert_eq!(archive.entries()[0].result.gauge, result.gauge());
            calls.borrow_mut().push("archive");
            Err(error(&archive_error))
        },
    )
    .unwrap_err();
    assert_eq!(*calls.borrow(), ["replay", "archive"]);
    assert!(Arc::ptr_eq(
        &returned.downcast_ref::<Marker>().unwrap().0,
        &output_error
    ));
    let calls = RefCell::new(Vec::new());
    crate::native_finish::finish_solo_with_result(
        Ok(Some(result)),
        Ok(()),
        Ok(()),
        None,
        None,
        &profile,
        None,
        |recording, path, failed| {
            assert!(recording.is_none());
            assert!(path.is_none());
            assert!(!failed);
            calls.borrow_mut().push("disabled");
            Ok(())
        },
        |_, _| {
            calls.borrow_mut().push("archive");
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(*calls.borrow(), ["disabled"]);
}
