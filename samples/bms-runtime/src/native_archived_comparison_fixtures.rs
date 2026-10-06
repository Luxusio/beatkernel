//! Deferred typed completion association over borrowed original-roster metadata.
use super::*;
use crate::{
    competition::ScoreSummary,
    competition_presentation::{
        CompetitionSnapshot, GhostSnapshot, NetworkSnapshot, NetworkStatus,
    },
    competition::OpponentKind,
    native_section_capture_fixtures::capture,
    gauge::BmsGauge,
    multiplayer::Progress,
    result_archive::{encode_archive, decode_archive, ArchivedScore},
};
use beatkernel::time::Timestamp;
fn result() -> CompletedPlayResult {
    CompletedPlayResult::from_completed(Timestamp::ZERO, None, &BmsGauge::default())
}
fn score() -> ScoreSummary {
    ScoreSummary {
        hits: 7,
        combo: 7,
        max_combo: 7,
        grades: [(u32::MAX, 7)].into_iter().collect(),
        ..Default::default()
    }
}
fn snapshot() -> CompetitionSnapshot {
    CompetitionSnapshot {
        ghosts: vec![GhostSnapshot {
            kind: OpponentKind::Other,
            label: "folder\\peer.bkr".into(),
            hits: 4,
            misses: 1,
            combo: 2,
            max_combo: 4,
            recorded_until: Some(Timestamp::from_nanos(604_800_000_000_000)),
        }],
        network: Some(NetworkSnapshot {
            status: NetworkStatus::Disconnected,
            progress: Some(Progress {
                song_ns: 9_007_199_254_740_993,
                hits: u64::MAX,
                misses: 0,
                combo: 0,
                max_combo: u64::MAX,
            }),
        }),
    }
}
#[test]
fn solo_builder_keeps_actual_typed_proof_capture_score_and_borrowed_comparison_without_prefix_promotion()
 {
    let recording = capture(0, None, u64::MAX);
    let gauge = BmsGauge::default();
    let score = score();
    let selected = snapshot();
    let outcome = Ok(Some(result()));
    let archive = solo_archive_with_score_and_comparisons(
        &outcome,
        Some(&recording),
        gauge.profile(),
        &score,
        &[(PlayerId(1), Some(&selected))],
    )
    .unwrap()
    .unwrap();
    assert_eq!(archive.entries()[0].header, *recording.header());
    assert_eq!(
        archive.entries()[0].score,
        Some(ArchivedScore::from_summary(&score).unwrap())
    );
    assert_eq!(archive.entries()[0].result.gauge, result().gauge());
    assert_eq!(
        archive.comparison(PlayerId(1)).unwrap().ghosts[0].label,
        "peer.bkr"
    );
    assert_eq!(
        archive.comparison(PlayerId(1)).unwrap().network,
        selected.network
    );
    assert_eq!(selected.ghosts[0].label, "folder\\peer.bkr");
    for prefix in [Ok(None), Err("prefix technical refusal".into())] {
        assert!(
            solo_archive_with_score_and_comparisons(
                &prefix,
                Some(&recording),
                gauge.profile(),
                &score,
                &[(PlayerId(0), Some(&selected))]
            )
            .unwrap()
            .is_none()
        );
    }
    assert!(
        solo_archive_with_score_and_comparisons(&outcome, None, gauge.profile(), &score, &[])
            .unwrap()
            .is_none()
    );
}
#[test]
fn publication_refusal_retains_exact_completion_and_v3_comparison_while_old_builder_stays_v2() {
    let recording = capture(0, None, 7);
    let gauge = BmsGauge::default();
    let score = score();
    let selected = snapshot();
    let outcome: NativeGameplayResult<Option<CompletedPlayResult>> =
        Err(Box::new(CompletedSoloPublicationError {
            result: result(),
            cause: "original publication refusal".into(),
        }));
    let archive = solo_archive_with_score_and_comparisons(
        &outcome,
        Some(&recording),
        gauge.profile(),
        &score,
        &[(PlayerId(1), Some(&selected))],
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        &encode_archive(&archive).unwrap()[8..12],
        &3u32.to_le_bytes()
    );
    assert_eq!(
        archive.entries()[0].result.gauge,
        outcome
            .as_ref()
            .unwrap_err()
            .downcast_ref::<CompletedSoloPublicationError>()
            .unwrap()
            .result
            .gauge()
    );
    let old = solo_archive_with_score(&outcome, Some(&recording), gauge.profile(), &score)
        .unwrap()
        .unwrap();
    assert!(old.comparisons().is_none());
    assert_eq!(&encode_archive(&old).unwrap()[8..12], &2u32.to_le_bytes());
}
#[test]
fn whole_one_two_and_sixty_four_original_ids_match_shuffled_members_scores_and_comparison_rows() {
    for count in [1u32, 2, 64] {
        let ids: Vec<_> = (0..count)
            .map(|index| PlayerId(u32::MAX - index * 17))
            .collect();
        let captures: Vec<_> = ids.iter().map(|id| capture(0, None, id.0 as u64)).collect();
        let gauge = BmsGauge::default();
        let score = score();
        let selected = snapshot();
        let members: Vec<_> = ids
            .iter()
            .zip(&captures)
            .rev()
            .map(|(id, capture)| ArchiveMember {
                player: *id,
                capture: Some(capture),
                profile: gauge.profile(),
            })
            .collect();
        let scores: Vec<_> = ids.iter().rev().map(|id| (*id, &score)).collect();
        let comparisons: Vec<_> = ids
            .iter()
            .enumerate()
            .rev()
            .map(|(index, id)| {
                (
                    *id,
                    if index % 2 == 0 {
                        Some(&selected)
                    } else {
                        None
                    },
                )
            })
            .collect();
        let outcome = Ok(Some(ids.iter().map(|id| (*id, result())).collect()));
        let archive =
            cohort_archive_with_scores_and_comparisons(&outcome, &members, &scores, &comparisons)
                .unwrap()
                .unwrap();
        assert_eq!(
            archive
                .comparisons()
                .unwrap()
                .iter()
                .map(|row| row.0)
                .collect::<Vec<_>>(),
            ids
        );
        for (index, id) in ids.iter().enumerate() {
            assert_eq!(archive.entries()[index].header, *captures[index].header());
            assert_eq!(
                archive.entries()[index].score,
                Some(ArchivedScore::from_summary(&score).unwrap())
            );
            assert_eq!(archive.comparison(*id).is_some(), index % 2 == 0);
            let member = archive.for_player(*id).unwrap();
            let decoded = decode_archive(&encode_archive(&member).unwrap()).unwrap();
            assert_eq!(
                decoded.comparisons(),
                Some(&[(*id, archive.comparison(*id).cloned())][..])
            );
        }
    }
}
#[test]
fn invalid_later_comparison_or_score_rows_refuse_whole_builder_but_keep_typed_local_proof() {
    let ids = [PlayerId(u32::MAX), PlayerId(7)];
    let recordings = [capture(0, None, 1), capture(0, None, 2)];
    let gauge = BmsGauge::default();
    let members: Vec<_> = ids
        .iter()
        .zip(&recordings)
        .map(|(id, capture)| ArchiveMember {
            player: *id,
            capture: Some(capture),
            profile: gauge.profile(),
        })
        .collect();
    let score = score();
    let mut bad_score = score.clone();
    bad_score.hits = 0;
    let mut selected = snapshot();
    selected.network.as_mut().unwrap().status = NetworkStatus::Waiting;
    let proof: Vec<_> = ids.iter().map(|id| (*id, result())).collect();
    let outcome: NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> =
        Err(Box::new(CompletedLocalPublicationError {
            results: proof.clone(),
            cause: "original local publication refusal".into(),
        }));
    for rows in [
        vec![],
        vec![(ids[0], None)],
        vec![(ids[0], None), (ids[0], None)],
        vec![(ids[0], None), (PlayerId(9), None)],
        vec![(ids[0], None), (ids[1], Some(&selected))],
    ] {
        assert!(
            cohort_archive_with_scores_and_comparisons(
                &outcome,
                &members,
                &[(ids[0], &score), (ids[1], &score)],
                &rows
            )
            .is_err()
        );
    }
    assert!(
        cohort_archive_with_scores_and_comparisons(
            &outcome,
            &members,
            &[(ids[0], &score), (ids[1], &bad_score)],
            &[(ids[0], None), (ids[1], None)]
        )
        .is_err()
    );
    assert_eq!(
        outcome
            .as_ref()
            .unwrap_err()
            .downcast_ref::<CompletedLocalPublicationError>()
            .unwrap()
            .results,
        proof
    );
}
