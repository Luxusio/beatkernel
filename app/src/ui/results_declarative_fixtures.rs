//! Production completed results and frozen display models on the mounted Results tree.
use super::*;
use crate::{
    competition_presentation::{GhostSnapshot, NetworkSnapshot},
    gauge::BmsGauge,
    ui::{
        interaction::Bounds,
        layout::{LayoutChange, LayoutUpdate, NodeId},
    },
};
use beatkernel::{
    chart::ObjectId,
    judge::{JudgeEvent, JudgeGrade, JudgeOutcome, JudgeStage},
    time::{Duration, Timestamp},
};

fn completed() -> CompletedPlayResult {
    CompletedPlayResult::from_completed(Timestamp::ZERO, None, &BmsGauge::default())
}

fn score() -> ScoreSummary {
    let mut summary = ScoreSummary::default();
    let events: Vec<_> = (0..9)
        .map(|index| JudgeEvent {
            object: ObjectId(index + 1),
            stage: JudgeStage::Instant,
            outcome: JudgeOutcome::Hit {
                grade: JudgeGrade(index as u32 + 1),
                delta: Duration::from_nanos(index as i64 - 4),
            },
            at: Timestamp::from_nanos(index as i64),
            input: None,
        })
        .collect();
    summary.observe(&events).unwrap();
    summary
}

fn competition() -> CompetitionSnapshot {
    CompetitionSnapshot {
        ghosts: vec![
            GhostSnapshot {
                kind: OpponentKind::Own,
                label: "my-original-record".into(),
                hits: 3,
                misses: 1,
                combo: 2,
                max_combo: 3,
                recorded_until: Some(Timestamp::from_nanos(550)),
            },
            GhostSnapshot {
                kind: OpponentKind::Other,
                label: "other-original-record".into(),
                hits: 4,
                misses: 0,
                combo: 4,
                max_combo: 4,
                recorded_until: None,
            },
        ],
        network: Some(NetworkSnapshot {
            status: NetworkStatus::Disconnected,
            progress: Some(crate::multiplayer::Progress {
                song_ns: 800,
                hits: 4,
                misses: 1,
                combo: 2,
                max_combo: 4,
            }),
        }),
    }
}

fn detailed(count: usize) -> ResultsView {
    let roster: Vec<_> = (0..count)
        .map(|index| PlayerId(u32::MAX - index as u32 * 17))
        .collect();
    let results: Vec<_> = roster
        .iter()
        .rev()
        .map(|&player| (player, completed()))
        .collect();
    let score = score();
    let competition = competition();
    let details: Vec<_> = roster
        .iter()
        .rev()
        .map(|&player| ResultDetails {
            player,
            score: &score,
            competition: Some(&competition),
        })
        .collect();
    ResultsView::new_with_details(&results, &roster, &details).unwrap()
}

fn geometry(scene: &Scene) -> Vec<([f32; 4], [f32; 4], [f32; 4])> {
    scene
        .rectangles()
        .iter()
        .map(|rectangle| (rectangle.bounds, rectangle.color, rectangle.uv))
        .collect()
}

fn render(view: &ResultsView, extent: [u32; 2], page: usize, comparisons: bool) -> Scene {
    let mut scene = Scene::with_capacity(extent[0], extent[1], 1024);
    view.compose_mode(&mut scene, page, comparisons).unwrap();
    scene
}

fn labels(scene: &Scene, label: &str) -> usize {
    let expected: Vec<_> = label.chars().map(crate::font::glyph_uv).collect();
    let actual: Vec<_> = scene
        .rectangles()
        .iter()
        .map(|rectangle| rectangle.uv)
        .collect();
    actual
        .windows(expected.len())
        .filter(|window| *window == expected.as_slice())
        .count()
}

fn layout_nodes(view: &ResultsView) -> Vec<NodeId> {
    view.layout.leaves().iter().map(|leaf| leaf.id).collect()
}

#[test]
fn production_score_and_completion_export_keep_every_page_and_frozen_pixel_parity() {
    let view = detailed(5);
    assert_eq!(
        view.rows().iter().map(|row| row.player).collect::<Vec<_>>(),
        (0..5)
            .map(|index| PlayerId(u32::MAX - index * 17))
            .collect::<Vec<_>>()
    );
    // Actual nine observed grade identities need two grade cards plus one summary
    // per player; two recorded prefixes and one peer prefix need three cards.
    assert_eq!(view.page_count_for(false), 4);
    assert_eq!(view.page_count_for(true), 4);
    let model = view.export_visual().unwrap();
    let frozen = FrozenResultsView::from_model(model.clone()).unwrap();
    assert_eq!(frozen.model(), &model);
    for comparisons in [false, true] {
        let mut totals = [0usize; 4];
        for page in 0..view.page_count_for(comparisons) {
            let scene = render(&view, [960, 720], page, comparisons);
            let mut copy = Scene::with_capacity(960, 720, 1024);
            frozen.compose_mode(&mut copy, page, comparisons).unwrap();
            assert_eq!(geometry(&copy), geometry(&scene));
            for (index, label) in [
                "GRADE G9 COUNT 1",
                "OWN RECORDED PREFIX",
                "OTHER RECORDED PREFIX",
                "SELF-REPORTED PEER PREFIX",
            ]
            .into_iter()
            .enumerate()
            {
                totals[index] += labels(&scene, label);
            }
            assert_eq!(labels(&scene, "FINAL RANK"), 0);
            assert_eq!(labels(&scene, "CLEARED"), 0);
        }
        assert_eq!(
            totals,
            if comparisons {
                [0, 5, 5, 5]
            } else {
                [5, 0, 0, 0]
            }
        );
    }
}

#[test]
fn result_details_are_frozen_from_real_observed_scores_before_caller_mutation() {
    let mut summary = score();
    let mut peers = competition();
    let saved_score = summary.clone();
    let saved_peers = peers.clone();
    let mut view = ResultsView::new_with_details(
        &[(PlayerId(7), completed())],
        &[PlayerId(7)],
        &[ResultDetails {
            player: PlayerId(7),
            score: &summary,
            competition: Some(&peers),
        }],
    )
    .unwrap();
    let saved_model = view.export_visual().unwrap();
    let saved_detail = geometry(&render(&view, [960, 720], 0, false));
    summary.hits = 0;
    summary.grades.clear();
    summary.timing = Default::default();
    peers.ghosts.clear();
    peers.network = None;
    assert_eq!(view.details()[0].score, saved_score);
    assert_eq!(view.details()[0].competition, Some(saved_peers));
    assert_eq!(view.export_visual().unwrap(), saved_model);
    assert!(!view.resize(960, 720).unwrap());
    assert_eq!(geometry(&render(&view, [960, 720], 0, false)), saved_detail);
    assert!(view.resize(700, 500).unwrap());
    assert_eq!(view.export_visual().unwrap(), saved_model);
    assert!(view.resize(960, 720).unwrap());
    assert_eq!(geometry(&render(&view, [960, 720], 0, false)), saved_detail);
}

#[test]
fn simple_results_preserve_roster_order_append_api_and_atomic_invalid_page_refusal() {
    let roster: Vec<_> = (1..=5).map(PlayerId).collect();
    let rows: Vec<_> = roster
        .iter()
        .rev()
        .map(|&player| (player, completed()))
        .collect();
    let view = ResultsView::new(&rows, &roster).unwrap();
    assert_eq!(view.page_count(), 2);
    assert!(!view.has_comparisons());
    let ids = layout_nodes(&view);
    let packet_storage = view.pages.as_ptr();
    let labels_storage: Vec<_> = view
        .rows()
        .iter()
        .map(|row| {
            (
                row.identity_label.as_ptr(),
                row.outcome_label.as_ptr(),
                row.gauge_label.as_ptr(),
            )
        })
        .collect();
    for page in [0, 1, 0, 1] {
        let mut scene = Scene::with_capacity(960, 720, 1024);
        scene.rect(1, 1, 2, 2, 0xffffff);
        let sentinel = geometry(&scene);
        view.compose_mode(&mut scene, page, true).unwrap();
        assert_eq!(
            &geometry(&scene)[..sentinel.len()],
            sentinel.as_slice(),
            "compose appends to caller-owned chrome"
        );
        let expected_player = if page == 0 { "PLAYER 1" } else { "PLAYER 5" };
        assert!(labels(&scene, expected_player) > 0);
        let accepted = geometry(&scene);
        let stamp = scene.geometry_stamp().1;
        for invalid in [2, usize::MAX] {
            assert!(view.compose_mode(&mut scene, invalid, true).is_err());
            assert_eq!(geometry(&scene), accepted);
            assert_eq!(scene.geometry_stamp().1, stamp);
        }
        assert_eq!(view.pages.as_ptr(), packet_storage);
        assert_eq!(layout_nodes(&view), ids);
        assert_eq!(
            view.rows()
                .iter()
                .map(|row| (
                    row.identity_label.as_ptr(),
                    row.outcome_label.as_ptr(),
                    row.gauge_label.as_ptr()
                ))
                .collect::<Vec<_>>(),
            labels_storage
        );
    }
}

#[test]
fn frozen_model_validation_keeps_foreign_mixed_scope_and_malformed_later_detail_refusal() {
    let view = detailed(2);
    let accepted = view.export_visual().unwrap();
    let mut variants = Vec::new();
    let mut duplicate = accepted.clone();
    duplicate.roster[1] = duplicate.roster[0];
    variants.push(duplicate);
    let mut foreign = accepted.clone();
    foreign.details[1].player = PlayerId(91);
    variants.push(foreign);
    let mut scope = accepted.clone();
    scope.rows[1].result.scope = PlayResultScope::PracticeSection {
        start: Timestamp::from_nanos(1),
        end: None,
    };
    variants.push(scope);
    let mut score = accepted.clone();
    score.details[1].score.grades[0].1 = 0;
    variants.push(score);
    let mut outcome = accepted.clone();
    outcome.rows[1].result.outcome = PlayResultOutcome::Failed(GaugeFailure::InstantDeath);
    variants.push(outcome);
    for malformed in variants {
        assert!(FrozenResultsView::from_model(malformed).is_err());
    }
    assert_eq!(view.export_visual().unwrap(), accepted);
    assert!(FrozenResultsView::from_model(accepted).is_ok());
}

#[test]
fn resized_and_suspended_results_share_visible_clips_without_mutating_frozen_data() {
    let mut view = detailed(2);
    let model = view.export_visual().unwrap();
    let mut frozen = FrozenResultsView::from_model(model.clone()).unwrap();
    let ids = layout_nodes(&view);
    let original = geometry(&render(&view, [960, 720], 0, false));
    assert!(view.resize(700, 500).unwrap());
    assert!(frozen.resize(700, 500).unwrap());
    for comparisons in [false, true] {
        let scene = render(&view, [700, 500], 0, comparisons);
        let mut reconstructed = Scene::with_capacity(700, 500, 1024);
        frozen
            .compose_mode(&mut reconstructed, 0, comparisons)
            .unwrap();
        assert_eq!(geometry(&scene), geometry(&reconstructed));
        assert!(scene.rectangles().iter().all(|r| r.bounds[0] >= 0.0
            && r.bounds[1] >= 0.0
            && r.bounds[0] + r.bounds[2] <= 700.0
            && r.bounds[1] + r.bounds[3] <= 500.0));
    }
    let packets = view.pages.as_ptr();
    assert!(!view.resize(700, 500).unwrap());
    assert_eq!(view.pages.as_ptr(), packets);
    assert!(view.resize(0, 500).unwrap());
    assert!(frozen.resize(0, 500).unwrap());
    let empty = render(&view, [0, 500], 0, false);
    assert!(empty.rectangles().is_empty());
    let mut chrome = Scene::with_capacity(960, 720, 16);
    chrome.rect(1, 1, 2, 2, 0xffffff);
    let sentinel = geometry(&chrome);
    view.compose_mode(&mut chrome, 0, true).unwrap();
    assert_eq!(
        geometry(&chrome),
        sentinel,
        "suspension cannot clear caller chrome"
    );
    assert!(view.compose_mode(&mut chrome, usize::MAX, true).is_err());
    assert!(view.resize(960, 720).unwrap());
    assert!(frozen.resize(960, 720).unwrap());
    assert_eq!(geometry(&render(&view, [960, 720], 0, false)), original);
    assert_eq!(layout_nodes(&view), ids);
    assert_eq!(view.export_visual().unwrap(), model);
    assert_eq!(frozen.model(), &model);
}

#[test]
fn card_child_reflow_clip_and_rejected_edit_publish_atomically_for_both_results_views() {
    let mut view = detailed(2);
    let model = view.export_visual().unwrap();
    let mut frozen = FrozenResultsView::from_model(model.clone()).unwrap();
    let first = view
        .layout
        .leaves()
        .iter()
        .find(|leaf| matches!(leaf.component, Component::Card(0)))
        .unwrap()
        .id;
    let second = view
        .layout
        .leaves()
        .iter()
        .find(|leaf| matches!(leaf.component, Component::Card(1)))
        .unwrap()
        .id;
    let before_y = view.layout.geometry(second).unwrap().bounds.y;
    let resize = [LayoutUpdate {
        id: first,
        change: LayoutChange::Size([912, 90]),
    }];
    assert!(view.update_layout(&resize).unwrap());
    assert!(frozen.update_layout(&resize).unwrap());
    assert_eq!(
        view.layout.geometry(second).unwrap().bounds.y,
        before_y - 10
    );
    let mut copy = Scene::with_capacity(960, 720, 1024);
    frozen.compose_mode(&mut copy, 0, false).unwrap();
    assert_eq!(
        geometry(&copy),
        geometry(&render(&view, [960, 720], 0, false))
    );
    let accepted = geometry(&copy);
    let packets = view.pages.as_ptr();
    let rejected = [LayoutUpdate {
        id: first,
        change: LayoutChange::Size([-1, 90]),
    }];
    assert!(view.update_layout(&rejected).is_err());
    assert!(frozen.update_layout(&rejected).is_err());
    assert_eq!(view.pages.as_ptr(), packets);
    assert_eq!(geometry(&render(&view, [960, 720], 0, false)), accepted);
    let clip = [LayoutUpdate {
        id: NodeId(0),
        change: LayoutChange::Clip(Some(Bounds {
            x: 24,
            y: 100,
            width: 300,
            height: 140,
        })),
    }];
    assert!(view.update_layout(&clip).unwrap());
    assert!(frozen.update_layout(&clip).unwrap());
    let clipped = render(&view, [960, 720], 0, false);
    assert!(!clipped.rectangles().is_empty());
    assert!(clipped.rectangles().iter().all(|r| r.bounds[0] >= 24.0
        && r.bounds[1] >= 100.0
        && r.bounds[0] + r.bounds[2] <= 324.0
        && r.bounds[1] + r.bounds[3] <= 240.0));
    let mut copy = Scene::with_capacity(960, 720, 1024);
    frozen.compose_mode(&mut copy, 0, false).unwrap();
    assert_eq!(geometry(&copy), geometry(&clipped));
    assert_eq!(view.export_visual().unwrap(), model);
    assert_eq!(frozen.model(), &model);
}
