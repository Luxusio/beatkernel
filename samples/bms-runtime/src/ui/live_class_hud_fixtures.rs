//! Actual scalar snapshots drive bounded live HUD geometry and original page identities.
use super::*;
use crate::{
    judgment_policy::BmsScoreSummary,
    local_players::PlayerId,
    play_policy::{ClassifiedWindow, ResolvedPlayPolicy},
    player::{self, PauseState},
};
use beatkernel::{
    judge::{JudgeGrade, JudgeWindow},
    time::Duration,
};

fn actual_snapshot(ids: &[PlayerId], classified: bool) -> crate::player::PlayerSnapshot {
    let source =
        beatkernel_bms::parse("#BPM 60\n#WAV01 x.wav\n#00011:01", Default::default()).unwrap();
    let compiled = source.compile().unwrap();
    let selected = ResolvedPlayPolicy::bms(
        &source,
        beatkernel_bms::BmsGaugeKind::Groove,
        &[ClassifiedWindow {
            judgment: beatkernel_bms::BmsJudgment::PGreat,
            window: JudgeWindow {
                grade: JudgeGrade(u32::MAX),
                early: Duration::ZERO,
                late: Duration::ZERO,
            },
        }],
        0,
    )
    .unwrap();
    let (publisher, viewer) = player::channel();
    let mut result = None;
    player::with_publisher(publisher, || {
        player::publish_local_chart(&source, &compiled.chart, ids).unwrap();
        if classified {
            player::prepare_native_play_policies(
                &ids.iter().map(|id| (*id, &selected)).collect::<Vec<_>>(),
            )
            .unwrap();
        }
        player::publish_pause(PauseState::Paused);
        player::publish_pause(PauseState::Running);
        result = viewer.take_latest();
        Ok(())
    })
    .unwrap();
    let mut snapshot = result.unwrap();
    for member in &mut snapshot.players {
        member.song_time = Some(Timestamp::ZERO);
    }
    snapshot
}
fn inside(rect: &[f32; 4], bounds: Bounds) -> bool {
    rect[0] >= bounds.x as f32
        && rect[1] >= bounds.y as f32
        && rect[0] + rect[2] <= (bounds.x + bounds.width) as f32
        && rect[1] + rect[3] <= (bounds.y + bounds.height) as f32
}
fn contains_text(scene: &Scene, bounds: Bounds, value: &str) -> bool {
    let glyphs = scene
        .rectangles()
        .iter()
        .filter(|r| inside(&r.bounds, bounds))
        .map(|r| r.uv)
        .collect::<Vec<_>>();
    let expected = value.chars().map(crate::font::glyph_uv).collect::<Vec<_>>();
    glyphs
        .windows(expected.len())
        .any(|window| window == expected)
}
fn same_geometry(a: &Scene, b: &Scene) {
    assert_eq!(a.rectangles().len(), b.rectangles().len());
    for (a, b) in a.rectangles().iter().zip(b.rectangles()) {
        assert_eq!(a.bounds, b.bounds);
        assert_eq!(a.color, b.color);
        assert_eq!(a.uv, b.uv);
    }
    assert_eq!(a.playfields().len(), b.playfields().len());
    for (a, b) in a.playfields().iter().zip(b.playfields()) {
        assert_eq!(a.top, b.top);
        assert_eq!(a.bottom, b.bottom);
    }
}
#[test]
fn actual_some_and_none_snapshot_bridge_preserves_scalar_and_legacy_geometry() {
    for classified in [false, true] {
        let snapshot = actual_snapshot(&[PlayerId(u32::MAX)], classified);
        let view = LocalPlayerView::from(&snapshot.players[0]);
        assert_eq!(view.bms_score.is_some(), classified);
        let mut scene = Scene::new(960, 720);
        local_players(&mut scene, &snapshot.players, 1_000_000_000, 0).unwrap();
        let field = &scene.playfields()[0];
        let legacy = crate::playfield_layout::local_field_bounds(1, 0).unwrap();
        assert_eq!(
            field.top,
            legacy[1] as f32 + 4.0 + if classified { 24.0 } else { 0.0 }
        );
        assert_eq!(field.bottom, (legacy[1] + legacy[3] - 9) as f32);
        let bounds = panel_bounds(0, 1);
        for label in ["EX", "PG", "G", "GOOD", "BAD", "POOR"] {
            assert_eq!(
                contains_text(
                    &scene,
                    Bounds {
                        x: bounds.x + 10,
                        y: bounds.y + 72,
                        width: bounds.width - 20,
                        height: 24
                    },
                    label
                ),
                classified,
                "{label}"
            );
        }
        assert!(scene.rectangles().iter().all(|r| inside(&r.bounds, bounds)));
        assert!(scene.status().is_ok());
    }
    let score = ScoreSummary::default();
    let mut old = Scene::new(960, 720);
    let mut new = Scene::new(960, 720);
    scoreboard(&mut old, &score, &[]);
    scoreboard_with_bms_score(&mut new, &score, &[], None);
    same_geometry(&old, &new);
    let competition = CompetitionSnapshot {
        ghosts: vec![],
        network: None,
    };
    old.clear();
    new.clear();
    competition_scoreboard(&mut old, &score, &competition).unwrap();
    competition_scoreboard_with_bms_score(&mut new, &score, &competition, None).unwrap();
    same_geometry(&old, &new);
}
#[test]
fn all_six_live_labels_large_scalars_and_sparse_page_ids_stay_in_each_visible_panel() {
    let ids = [
        PlayerId(7),
        PlayerId(99),
        PlayerId(u32::MAX),
        PlayerId(501),
        PlayerId(9001),
        PlayerId(42),
        PlayerId(88),
    ];
    let mut snapshot = actual_snapshot(&ids, true);
    for member in &mut snapshot.players {
        member.song_time = Some(Timestamp::ZERO);
        member.score.hits = u64::MAX;
        member.score.misses = 0;
        member.bms_score = Some(BmsScoreSummary {
            pgreat: 0,
            great: u64::MAX,
            good: 0,
            bad: 0,
            poor: 0,
            ex_score: u64::MAX,
        });
    }
    for count in 1..=4 {
        let mut scene = Scene::new(960, 720);
        local_players(&mut scene, &snapshot.players[..count], 1_000_000_000, 0).unwrap();
        for (slot, member) in snapshot.players[..count].iter().enumerate() {
            let panel = panel_bounds(slot, count);
            let classes = Bounds {
                x: panel.x + 10,
                y: panel.y + 72,
                width: panel.width - 20,
                height: 24,
            };
            for label in ["EX", "PG", "G", "GOOD", "BAD", "POOR"] {
                assert!(
                    contains_text(&scene, classes, label),
                    "missing {label} in {count} panel layout"
                );
            }
            assert!(contains_text(
                &scene,
                Bounds {
                    x: panel.x + 10,
                    y: panel.y + 8,
                    width: panel.width - 20,
                    height: 14
                },
                &format!("P{}", member.player.0)
            ));
            assert!(
                scene
                    .rectangles()
                    .iter()
                    .filter(|r| r.bounds[1] >= classes.y as f32
                        && r.bounds[1] < (classes.y + 24) as f32
                        && r.bounds[0] >= panel.x as f32
                        && r.bounds[0] < (panel.x + panel.width) as f32)
                    .all(|r| inside(&r.bounds, classes))
            );
        }
        assert!(
            scene
                .rectangles()
                .iter()
                .all(|r| (0..count).any(|slot| inside(&r.bounds, panel_bounds(slot, count))))
        );
    }
    let mut scene = Scene::new(960, 720);
    local_players(&mut scene, &snapshot.players, 1_000_000_000, 1).unwrap();
    for (slot, member) in snapshot.players[4..].iter().enumerate() {
        let panel = panel_bounds(slot, 3);
        assert!(contains_text(
            &scene,
            Bounds {
                x: panel.x + 10,
                y: panel.y + 8,
                width: panel.width - 20,
                height: 14
            },
            &format!("P{}", member.player.0)
        ));
    }
    assert_eq!(scene.playfields().len(), 3);
    for member in &mut snapshot.players {
        member.score.hits = 0;
        member.score.misses = u64::MAX;
        member.bms_score = Some(BmsScoreSummary {
            poor: u64::MAX,
            ..Default::default()
        });
    }
    for count in 1..=4 {
        scene.clear();
        local_players(&mut scene, &snapshot.players[..count], 1_000_000_000, 0).unwrap();
        for slot in 0..count {
            let panel = panel_bounds(slot, count);
            assert!(contains_text(
                &scene,
                Bounds {
                    x: panel.x + 10,
                    y: panel.y + 72,
                    width: panel.width - 20,
                    height: 24
                },
                "POOR"
            ));
        }
        assert!(
            scene
                .rectangles()
                .iter()
                .all(|r| (0..count).any(|slot| inside(&r.bounds, panel_bounds(slot, count))))
        );
    }
}
#[test]
fn gauge_class_comparison_and_playfield_regions_are_disjoint_with_fixed_reservations() {
    for count in 1..=4 {
        let ids = (0..count)
            .map(|i| PlayerId(7 + i as u32 * 10))
            .collect::<Vec<_>>();
        let mut snapshot = actual_snapshot(&ids, true);
        for member in &mut snapshot.players {
            member.song_time = Some(Timestamp::ZERO);
            member.competition = Some(CompetitionSnapshot {
                ghosts: vec![],
                network: Some(crate::player::NetworkSnapshot {
                    status: crate::player::NetworkStatus::Connected,
                    progress: Some(crate::multiplayer::Progress {
                        song_ns: 0,
                        hits: 123,
                        misses: 0,
                        combo: 1,
                        max_combo: 1,
                    }),
                }),
            });
        }
        let views = snapshot
            .players
            .iter()
            .map(LocalPlayerView::from)
            .collect::<Vec<_>>();
        let mut scene = Scene::new(960, 720);
        local_player_views_with_reserved_comparison_space(
            &mut scene,
            &views,
            1_000_000_000,
            0,
            true,
            &[crate::bga_render::BgaFrame::default(); 4],
            &vec![28; count],
        )
        .unwrap();
        for slot in 0..count {
            let panel = panel_bounds(slot, count);
            let field = &scene.playfields()[slot];
            let legacy = crate::playfield_layout::local_field_bounds(count, slot).unwrap();
            assert_eq!(field.top, legacy[1] as f32 + 4.0 + 24.0 + 28.0);
            assert_eq!(field.bottom, (legacy[1] + legacy[3] - 9) as f32);
            assert!(contains_text(
                &scene,
                Bounds {
                    x: panel.x + 10,
                    y: panel.y + 96,
                    width: panel.width - 20,
                    height: 28
                },
                "PEER"
            ));
            assert!(contains_text(
                &scene,
                Bounds {
                    x: panel.x + 10,
                    y: panel.y + 72,
                    width: panel.width - 20,
                    height: 24
                },
                "POOR"
            ));
            assert!(
                scene
                    .rectangles()
                    .iter()
                    .filter(|r| r.bounds[1] >= panel.y as f32 + 56.0
                        && r.bounds[1] < panel.y as f32 + 70.0
                        && r.bounds[0] >= panel.x as f32
                        && r.bounds[0] <= (panel.x + panel.width) as f32)
                    .all(|r| r.bounds[1] + r.bounds[3] <= panel.y as f32 + 70.0)
            );
        }
    }
}
