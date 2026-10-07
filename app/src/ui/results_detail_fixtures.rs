//! Deferred frozen final score/comparison views, independent of IO and time.
use super::*;
use crate::{
    competition::{OpponentKind, ScoreSummary},
    competition_presentation::{
        CompetitionSnapshot, GhostSnapshot, NetworkSnapshot, NetworkStatus,
    },
    gauge::BmsGauge,
    play_result::CompletedPlayResult,
};
use beatkernel::{
    chart::ObjectId,
    judge::{JudgeEvent, JudgeGrade, JudgeOutcome, JudgeStage},
    time::{Duration, Timestamp},
};

fn completed(end: Option<i64>) -> CompletedPlayResult {
    CompletedPlayResult::from_completed(
        Timestamp::ZERO,
        end.map(Timestamp::from_nanos),
        &BmsGauge::default(),
    )
}
fn score() -> ScoreSummary {
    let mut score = ScoreSummary::default();
    let events = [-3, 0, 5]
        .into_iter()
        .enumerate()
        .map(|(index, delta)| JudgeEvent {
            object: ObjectId(index as u64 + 1),
            stage: JudgeStage::Instant,
            outcome: JudgeOutcome::Hit {
                grade: JudgeGrade(7),
                delta: Duration::from_nanos(delta),
            },
            at: Timestamp::from_nanos(index as i64),
            input: None,
        })
        .collect::<Vec<_>>();
    score.observe(&events).unwrap();
    score.hits = u64::MAX;
    score.combo = u64::MAX;
    score.max_combo = u64::MAX;
    score
}
fn comparisons(count: usize) -> CompetitionSnapshot {
    CompetitionSnapshot {
        ghosts: (0..count)
            .map(|index| GhostSnapshot {
                kind: if index % 2 == 0 {
                    OpponentKind::Own
                } else {
                    OpponentKind::Other
                },
                label: format!("record-{index}-{}", "X".repeat(50)),
                hits: u64::MAX,
                misses: 0,
                combo: u64::MAX,
                max_combo: u64::MAX,
                recorded_until: (index != 0).then_some(Timestamp::from_nanos(604_800_000_000_001)),
            })
            .collect(),
        network: Some(NetworkSnapshot {
            status: NetworkStatus::Disconnected,
            progress: Some(crate::multiplayer::Progress {
                song_ns: i64::MIN,
                hits: u64::MAX,
                misses: 0,
                combo: u64::MAX,
                max_combo: u64::MAX,
            }),
        }),
    }
}

fn label_count(scene: &Scene, label: &str) -> usize {
    // Literal text oracle against the actual emitted bitmap glyph geometry.
    let expected = label.chars().map(crate::font::glyph_uv).collect::<Vec<_>>();
    let actual = scene
        .rectangles()
        .iter()
        .map(|rectangle| rectangle.uv)
        .collect::<Vec<_>>();
    actual
        .windows(expected.len())
        .filter(|window| *window == expected.as_slice())
        .count()
}
fn assert_label(scene: &Scene, label: &str) {
    assert!(
        label_count(scene, label) > 0,
        "missing literal label: {label}"
    );
}

#[test]
fn actual_details_freeze_exact_counters_known_timing_and_comparison_extents() {
    let result = completed(None);
    let mut score = score();
    let mut competition = comparisons(2);
    let saved_score = score.clone();
    let saved_competition = competition.clone();
    let view = ResultsView::new_with_details(
        &[(PlayerId(u32::MAX), result)],
        &[PlayerId(u32::MAX)],
        &[ResultDetails {
            player: PlayerId(u32::MAX),
            score: &score,
            competition: Some(&competition),
        }],
    )
    .unwrap();
    score.hits = 0;
    score.timing = Default::default();
    competition.ghosts.clear();
    competition.network = None;
    assert_eq!(view.details()[0].score, saved_score);
    assert_eq!(view.details()[0].competition, Some(saved_competition));
    assert_eq!(view.details()[0].score.timing.count(), 3);
    assert_eq!(view.details()[0].score.timing.mean_ns(), Some(0));
    assert_eq!(view.details()[0].score.timing.mean_absolute_ns(), Some(2));
    let mut scene = Scene::new(960, 720);
    view.compose_mode(&mut scene, 0, false).unwrap();
    assert_label(
        &scene,
        "HITS 18446744073709551615 MISSES 0 COMBO 18446744073709551615 MAX COMBO 18446744073709551615",
    );
    assert_label(&scene, "BIAS 0.000 MS");
    assert_label(&scene, "MEAN ABS 0.000 MS");
    scene.clear();
    view.compose_mode(&mut scene, 0, true).unwrap();
    for label in [
        "OWN RECORDED PREFIX",
        "OTHER RECORDED PREFIX",
        "RECORDED UNTIL UNKNOWN",
        "RECORDED UNTIL 604800000000001 NS",
        "SELF-REPORTED PEER PREFIX",
        "DISCONNECTED",
        "PREFIX SONG -9223372036854775808 NS",
    ] {
        assert_label(&scene, label);
    }
    assert_eq!(label_count(&scene, "CLEARED"), 0);
    assert_eq!(label_count(&scene, "FINAL RANK"), 0);
}

#[test]
fn detail_validation_is_whole_roster_atomic_including_later_foreign_or_duplicate_rows() {
    let result = completed(None);
    let score = score();
    let competition = comparisons(2);
    let roster = [PlayerId(7), PlayerId(u32::MAX)];
    let results = [(roster[0], result), (roster[1], result)];
    let valid = [
        ResultDetails {
            player: roster[1],
            score: &score,
            competition: Some(&competition),
        },
        ResultDetails {
            player: roster[0],
            score: &score,
            competition: None,
        },
    ];
    let view = ResultsView::new_with_details(&results, &roster, &valid).unwrap();
    assert_eq!(
        view.details()
            .iter()
            .map(|row| row.player)
            .collect::<Vec<_>>(),
        roster
    );
    for details in [
        vec![],
        vec![ResultDetails {
            player: roster[0],
            score: &score,
            competition: None,
        }],
        vec![
            ResultDetails {
                player: roster[0],
                score: &score,
                competition: None,
            },
            ResultDetails {
                player: PlayerId(9),
                score: &score,
                competition: None,
            },
        ],
        vec![
            ResultDetails {
                player: roster[0],
                score: &score,
                competition: None,
            },
            ResultDetails {
                player: roster[0],
                score: &score,
                competition: None,
            },
        ],
    ] {
        assert!(ResultsView::new_with_details(&results, &roster, &details).is_err());
    }
    let too_many = comparisons(9);
    assert!(
        ResultsView::new_with_details(
            &results,
            &roster,
            &[
                ResultDetails {
                    player: roster[0],
                    score: &score,
                    competition: None
                },
                ResultDetails {
                    player: roster[1],
                    score: &score,
                    competition: Some(&too_many)
                },
            ]
        )
        .is_err()
    );
    let mixed = [(roster[0], result), (roster[1], completed(Some(10)))];
    assert!(ResultsView::new_with_details(&mixed, &roster, &valid).is_err());
    assert_eq!(view.details()[1].competition, Some(competition));
}

#[test]
fn all_supported_comparisons_are_reachable_and_every_emitted_glyph_stays_in_bounds() {
    let roster = (0..64)
        .map(|index| PlayerId(u32::MAX - index * 17))
        .collect::<Vec<_>>();
    let result = completed(Some(604_800_000_000_001));
    let results = roster.iter().map(|&id| (id, result)).collect::<Vec<_>>();
    let score = score();
    let competition = comparisons(8);
    let details = roster
        .iter()
        .rev()
        .map(|&player| ResultDetails {
            player,
            score: &score,
            competition: Some(&competition),
        })
        .collect::<Vec<_>>();
    let view = ResultsView::new_with_details(&results, &roster, &details).unwrap();
    assert!(view.has_comparisons());
    // One detail card plus one known-grade card for each original player.
    assert_eq!(view.page_count_for(false), 32);
    assert_eq!(view.page_count_for(true), 144);
    let mut totals = [0, 0, 0];
    let mut scene = Scene::new(960, 720);
    for page in 0..144 {
        scene.clear();
        view.compose_mode(&mut scene, page, true).unwrap();
        scene.status().unwrap();
        assert!(scene.playfields().is_empty());
        for (slot, label) in [
            "OWN RECORDED PREFIX",
            "OTHER RECORDED PREFIX",
            "SELF-REPORTED PEER PREFIX",
        ]
        .into_iter()
        .enumerate()
        {
            totals[slot] += label_count(&scene, label);
        }
        for rectangle in scene.rectangles() {
            let [x, y, width, height] = rectangle.bounds;
            assert!(x >= 24.0 && x + width <= 936.0 && y >= 0.0 && y + height <= 720.0);
        }
    }
    assert_eq!(totals, [256, 256, 64]);
    assert_eq!(view.details()[63].player, roster[63]);
    assert_eq!(
        view.details()[63]
            .competition
            .as_ref()
            .unwrap()
            .ghosts
            .len(),
        8
    );
}

#[test]
fn mode_composition_reuses_retained_pages_and_invalid_bounds_leave_scene_unchanged() {
    let score = score();
    let competition = comparisons(8);
    let result = completed(None);
    let view = ResultsView::new_with_details(
        &[(PlayerId(7), result)],
        &[PlayerId(7)],
        &[ResultDetails {
            player: PlayerId(7),
            score: &score,
            competition: Some(&competition),
        }],
    )
    .unwrap();
    let detail_packets = view.pages.as_ptr();
    let comparison_packets = view.comparison_pages.as_ptr();
    let identity = view.rows()[0].identity_label.as_ptr();
    let mut scene = Scene::new(960, 720);
    for (page, comparisons) in [(0, false), (2, true), (0, false), (0, true)] {
        scene.clear();
        view.compose_mode(&mut scene, page, comparisons).unwrap();
        assert_eq!(view.pages.as_ptr(), detail_packets);
        assert_eq!(view.comparison_pages.as_ptr(), comparison_packets);
        assert_eq!(view.rows()[0].identity_label.as_ptr(), identity);
    }
    for (page, comparisons) in [(1, false), (3, true), (usize::MAX, true)] {
        let epoch = scene.geometry_stamp().1;
        assert!(view.compose_mode(&mut scene, page, comparisons).is_err());
        assert_eq!(scene.geometry_stamp().1, epoch);
    }
    let empty = ResultsView::new_with_details(
        &[(PlayerId(7), result)],
        &[PlayerId(7)],
        &[ResultDetails {
            player: PlayerId(7),
            score: &score,
            competition: None,
        }],
    )
    .unwrap();
    assert!(!empty.has_comparisons());
    assert_eq!(empty.page_count_for(true), empty.page_count_for(false));
    scene.clear();
    empty.compose_mode(&mut scene, 0, true).unwrap();
    assert_label(&scene, "HITS 18446744073709551615");
}

#[test]
fn opaque_grade_counts_and_unknown_peer_prefix_remain_explicit_without_clear_claims() {
    let mut score = score();
    score.grades = (0..9)
        .map(|index| (u32::MAX - index * 17, u64::MAX))
        .collect();
    let competition = CompetitionSnapshot {
        ghosts: vec![],
        network: Some(NetworkSnapshot {
            status: NetworkStatus::Waiting,
            progress: None,
        }),
    };
    let result = completed(Some(10));
    let view = ResultsView::new_with_details(
        &[(PlayerId(7), result)],
        &[PlayerId(7)],
        &[ResultDetails {
            player: PlayerId(7),
            score: &score,
            competition: Some(&competition),
        }],
    )
    .unwrap();
    let mut scene = Scene::new(960, 720);
    view.compose_mode(&mut scene, 0, false).unwrap();
    for index in 0..9 {
        assert_label(
            &scene,
            &format!(
                "GRADE G{} COUNT 18446744073709551615",
                u32::MAX - index * 17
            ),
        );
    }
    scene.clear();
    view.compose_mode(&mut scene, 0, true).unwrap();
    assert_label(&scene, "PEER PREFIX UNAVAILABLE");
    assert_label(&scene, "NETWORK WAITING");
    assert_eq!(label_count(&scene, "PREFIX SONG"), 0);
    assert_eq!(label_count(&scene, "CLEARED"), 0);
    assert!(!result.whole_song_clear());
}
