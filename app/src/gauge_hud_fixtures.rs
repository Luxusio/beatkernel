//! Deferred CPU Scene geometry only; no renderer/device or visual acceptance.
#![cfg(feature = "graphics")]

use crate::{
    bga_render::BgaFrame,
    browser_input::TouchInputSetup,
    competition::ScoreSummary,
    gauge::{BmsGauge, GaugeFailure, GaugeProfile},
    local_players::PlayerId,
    note_progress::NoteProgress,
    player::{CompetitionSnapshot, LocalPlayerSnapshot, NetworkSnapshot, NetworkStatus},
    player_chart::PlayerChart,
    playfield_layout::{
        default_touch_bounds, local_field_bounds_with_comparison_space,
        local_touch_bounds_with_comparison_space,
    },
    scene::{Rectangle, Scene},
    texture::TextureId,
    ui::{
        interaction::Bounds,
        molecules::gauge_hud,
        organisms::{self, LocalPlayerView},
    },
};
use beatkernel::{
    chart::ObjectId,
    input::{
        BackendId, ContactId, DeviceId, EventMeta, GameControlId, PhysicalControlId,
        PhysicalInputEvent, Position2, TouchEvent, TouchPhase, TouchRoute,
    },
    judge::{
        HazardEvent, HazardId, HazardOutcome, JudgeEvent, JudgeGrade, JudgeOutcome, JudgeStage,
    },
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use std::sync::Arc;

fn gauge_at(units: u64) -> BmsGauge {
    BmsGauge::new(
        GaugeProfile::new(units, 80_000_000, 1_000_000, -6_000_000, false, vec![]).unwrap(),
    )
}
fn color(rgb: u32) -> [f32; 4] {
    [
        ((rgb >> 16) & 255) as f32 / 255.0,
        ((rgb >> 8) & 255) as f32 / 255.0,
        (rgb & 255) as f32 / 255.0,
        1.0,
    ]
}
fn rows(scene: &Scene, texture: TextureId) -> Vec<&Rectangle> {
    scene
        .batches()
        .iter()
        .filter(|batch| batch.texture == texture && batch.playfield.is_none())
        .flat_map(|batch| {
            &scene.rectangles()[batch.first as usize..(batch.first + batch.count) as usize]
        })
        .collect()
}
fn geometry(scene: &Scene) -> Vec<([u32; 4], [u32; 4], [u32; 4])> {
    scene
        .rectangles()
        .iter()
        .map(|r| {
            (
                r.bounds.map(f32::to_bits),
                r.color.map(f32::to_bits),
                r.uv.map(f32::to_bits),
            )
        })
        .collect()
}
fn label_at(scene: &Scene, expected: &str, x: i64, y: i64, rgb: u32) {
    let glyphs: Vec<_> = rows(scene, TextureId::FONT)
        .into_iter()
        .filter(|r| {
            r.bounds[1] == y as f32
                && r.bounds[0] >= x as f32
                && r.bounds[0] < (x + expected.len() as i64 * 6) as f32
        })
        .collect();
    assert_eq!(glyphs.len(), expected.len());
    for (index, (glyph, character)) in glyphs.iter().zip(expected.chars()).enumerate() {
        assert_eq!(
            glyph.bounds,
            [(x + index as i64 * 6) as f32, y as f32, 5.0, 7.0]
        );
        assert_eq!(glyph.uv, crate::font::glyph_uv(character));
        assert_eq!(glyph.color, color(rgb));
    }
}
fn mine(value: u64) -> HazardEvent {
    HazardEvent {
        id: HazardId(9),
        at: Timestamp::ZERO,
        control: GameControlId(0x11),
        value,
        outcome: HazardOutcome::Triggered,
        input: None,
    }
}

#[test]
fn integer_percentages_truncate_at_exact_boundaries_and_bar_geometry_has_a_constant_budget() {
    let bounds = Bounds {
        x: 7,
        y: 9,
        width: 103,
        height: 18,
    };
    for (units, label, filled, rgb) in [
        (0, "GAUGE 0.00%", 0, 0x4f92db),
        (1, "GAUGE 0.00%", 0, 0x4f92db),
        (9_999, "GAUGE 0.00%", 0, 0x4f92db),
        (10_000, "GAUGE 0.01%", 0, 0x4f92db),
        (999_999, "GAUGE 0.99%", 1, 0x4f92db),
        (1_000_000, "GAUGE 1.00%", 1, 0x4f92db),
        (19_999_999, "GAUGE 19.99%", 20, 0x4f92db),
        (20_000_000, "GAUGE 20.00%", 20, 0x4f92db),
        (79_999_999, "GAUGE 79.99%", 82, 0x4f92db),
        (80_000_000, "READY 80.00%", 82, 0x61d69a),
        (99_999_999, "READY 99.99%", 102, 0x61d69a),
        (100_000_000, "READY 100.00%", 103, 0x61d69a),
    ] {
        let gauge = gauge_at(units);
        let before = gauge.clone();
        let mut scene = Scene::new(200, 80);
        gauge_hud(&mut scene, &gauge, bounds).unwrap();
        scene.status().unwrap();
        let bars = rows(&scene, TextureId::WHITE);
        assert_eq!(bars[0].bounds, [7.0, 9.0, 103.0, 18.0]);
        assert_eq!(bars[0].color, color(0x1f2d40));
        assert_eq!(bars.len(), if filled == 0 { 1 } else { 2 });
        if filled != 0 {
            assert_eq!(bars[1].bounds, [7.0, 23.0, filled as f32, 4.0]);
            assert_eq!(bars[1].color, color(rgb));
        }
        label_at(&scene, label, 9, 11, rgb);
        assert_eq!(scene.rectangles().len(), label.len() + bars.len());
        assert!(scene.rectangles().len() <= 15);
        assert!(scene.batches().len() <= 2 && scene.playfields().is_empty());
        assert_eq!(gauge, before);
    }
    let mut scene = Scene::new(200, 40);
    gauge_hud(
        &mut scene,
        &gauge_at(50_000_000),
        Bounds {
            x: 0,
            y: 0,
            width: i64::MAX,
            height: 14,
        },
    )
    .unwrap();
    let bars = rows(&scene, TextureId::WHITE);
    assert_eq!(bars[0].bounds, [0.0, 0.0, 200.0, 14.0]);
    assert_eq!(bars[1].bounds, [0.0, 10.0, 200.0, 4.0]);
    label_at(&scene, "GAUGE 50.00%", 2, 2, 0x4f92db);
}

#[test]
fn readiness_recoverable_zero_and_both_latched_failures_have_distinct_read_only_text_and_color() {
    let mut recovered = BmsGauge::default();
    recovered.observe(&[], &[mine(50)]).unwrap();
    let depleted =
        BmsGauge::new(GaugeProfile::new(0, 0, 1_000_000, -6_000_000, true, vec![]).unwrap());
    let mut dead = BmsGauge::default();
    dead.observe(&[], &[mine(1295)]).unwrap();
    assert_eq!(depleted.snapshot().failure, Some(GaugeFailure::Depleted));
    assert_eq!(dead.snapshot().failure, Some(GaugeFailure::InstantDeath));
    let ready = gauge_at(80_000_000);
    assert!(ready.can_clear());
    for (gauge, label, rgb) in [
        (recovered, "GAUGE 0.00%", 0x4f92db),
        (depleted, "EMPTY 0.00%", 0xd8b36b),
        (dead, "DEAD 0.00%", 0xef6372),
        (ready, "READY 80.00%", 0x61d69a),
    ] {
        let before = gauge.clone();
        let mut scene = Scene::new(240, 80);
        let bounds = Bounds {
            x: 10,
            y: 10,
            width: 186,
            height: 18,
        };
        gauge_hud(&mut scene, &gauge, bounds).unwrap();
        label_at(&scene, label, 12, 12, rgb);
        let prior = geometry(&scene);
        for _ in 0..3 {
            scene.clear();
            gauge_hud(&mut scene, &gauge, bounds).unwrap();
            assert_eq!(geometry(&scene), prior);
            assert_eq!(gauge, before);
        }
        if gauge.snapshot().failure.is_some() {
            assert!(!gauge.can_clear());
        }
    }
    // READY is only the observed level predicate: this component receives no
    // song, replay cursor, output drain, or completion owner to advance.
}

#[test]
fn invalid_bounds_leave_existing_geometry_untouched_and_tiny_or_edge_labels_stay_clipped() {
    let gauge = BmsGauge::default();
    let before = gauge.clone();
    let mut scene = Scene::new(200, 50);
    scene.rect(1, 2, 3, 4, 0x010203);
    let initial = geometry(&scene);
    let epoch = scene.geometry_stamp().1;
    let batches: Vec<_> = scene
        .batches()
        .iter()
        .map(|b| (b.texture, b.first, b.count, b.playfield))
        .collect();
    for bounds in [
        Bounds {
            x: -1,
            y: 0,
            width: 100,
            height: 14,
        },
        Bounds {
            x: 0,
            y: -1,
            width: 100,
            height: 14,
        },
        Bounds {
            x: 0,
            y: 0,
            width: 0,
            height: 14,
        },
        Bounds {
            x: 0,
            y: 0,
            width: -1,
            height: 14,
        },
        Bounds {
            x: 0,
            y: 0,
            width: 100,
            height: 13,
        },
        Bounds {
            x: 0,
            y: 0,
            width: 100,
            height: i64::MIN,
        },
        Bounds {
            x: i64::MAX,
            y: 0,
            width: 1,
            height: 14,
        },
        Bounds {
            x: 0,
            y: i64::MAX - 13,
            width: 100,
            height: 14,
        },
        Bounds {
            x: i64::MAX - 1,
            y: 0,
            width: 1,
            height: 14,
        },
    ] {
        assert!(gauge_hud(&mut scene, &gauge, bounds).is_err());
        assert_eq!(geometry(&scene), initial);
        assert_eq!(scene.geometry_stamp().1, epoch);
        assert_eq!(
            scene
                .batches()
                .iter()
                .map(|b| (b.texture, b.first, b.count, b.playfield))
                .collect::<Vec<_>>(),
            batches
        );
        scene.status().unwrap();
        assert_eq!(gauge, before);
    }
    for bounds in [
        Bounds {
            x: 5,
            y: 5,
            width: 1,
            height: 14,
        },
        Bounds {
            x: 5,
            y: 5,
            width: 3,
            height: 14,
        },
        Bounds {
            x: 5,
            y: 5,
            width: 7,
            height: 14,
        },
        Bounds {
            x: 5,
            y: 5,
            width: 33,
            height: 14,
        },
        Bounds {
            x: 199,
            y: 39,
            width: 100,
            height: 14,
        },
        Bounds {
            x: 200,
            y: 50,
            width: 100,
            height: 14,
        },
    ] {
        scene.clear();
        gauge_hud(&mut scene, &gauge, bounds).unwrap();
        for rect in scene.rectangles() {
            let [x, y, w, h] = rect.bounds;
            assert!(x >= bounds.x as f32 && y >= bounds.y as f32 && w > 0.0 && h > 0.0);
            assert!(x + w <= (bounds.x + bounds.width).min(200) as f32);
            assert!(y + h <= (bounds.y + bounds.height).min(50) as f32);
        }
        assert!(scene.rectangles().len() <= 15);
        assert_eq!(gauge, before);
    }
    scene.clear();
    gauge_hud(
        &mut scene,
        &gauge,
        Bounds {
            x: 5,
            y: 5,
            width: 3,
            height: 14,
        },
    )
    .unwrap();
    let glyphs = rows(&scene, TextureId::FONT);
    assert_eq!(glyphs.len(), 1);
    assert_eq!(glyphs[0].bounds, [7.0, 7.0, 1.0, 7.0]);
    let original = crate::font::glyph_uv('G');
    assert_eq!(glyphs[0].uv[0], original[0]);
    assert_eq!(glyphs[0].uv[1], original[1]);
    assert!((glyphs[0].uv[2] - original[2] / 5.0).abs() < 0.000001);
}

fn hit(index: u64) -> JudgeEvent {
    JudgeEvent {
        object: ObjectId(index),
        stage: JudgeStage::Instant,
        outcome: JudgeOutcome::Hit {
            grade: JudgeGrade(1),
            delta: Duration::ZERO,
        },
        at: Timestamp::ZERO,
        input: None,
    }
}
fn touch() -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(
            DeviceId(3),
            ClockPoint {
                domain: ClockDomainId(7),
                timestamp: Timestamp::ZERO,
            },
            u64::MAX,
        ),
        control: PhysicalControlId::Native {
            backend: BackendId(0x5754_4f55),
            code: 0,
        },
        contact: ContactId(u64::MAX),
        phase: TouchPhase::Down,
        position: Position2 { x: 0.25, y: 0.5 },
        pressure: Some(0.5),
    })
}

#[test]
fn local_snapshot_pages_borrow_independent_gauges_without_changing_fields_touch_or_comparison_reservations()
 {
    let source = beatkernel_bms::parse(
        "#BPM 60\n#WAV01 tone\n#00011:01\n#00012:0001",
        Default::default(),
    )
    .unwrap();
    let chart =
        Arc::new(PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap());
    let players: Vec<_> = (0..64)
        .map(|index| {
            // These are public retained view inputs, not a native device session.
            // Vary actual shared-policy observations to expose accidental aliasing.
            let events: Vec<_> = (0..index).map(hit).collect();
            let mut score = ScoreSummary::default();
            score.observe(&events).unwrap();
            let mut gauge = BmsGauge::default();
            gauge.observe(&events, &[]).unwrap();
            LocalPlayerSnapshot {
                bms_score: None,
                player: PlayerId(if index == 63 {
                    u32::MAX
                } else {
                    index as u32 * 3 + 1
                }),
                chart: Some(Arc::clone(&chart)),
                song_time: Some(Timestamp::ZERO),
                score,
                mine_damage: Default::default(),
                gauge,
                last_judge: Some(hit(1)),
                recent_results: vec![],
                pressed_lanes: 0,
                note_progress: Some(NoteProgress::new(Arc::clone(&chart)).unwrap()),
                competition: Some(CompetitionSnapshot {
                    ghosts: vec![],
                    network: Some(NetworkSnapshot {
                        status: NetworkStatus::Connected,
                        progress: None,
                    }),
                }),
            }
        })
        .collect();
    let retained = players.clone();
    let views: Vec<_> = players.iter().map(LocalPlayerView::from).collect();
    for (view, player) in views.iter().zip(&players) {
        assert!(std::ptr::eq(view.gauge.unwrap(), &player.gauge));
    }
    let mut preview = views.clone();
    for view in &mut preview {
        view.gauge = None;
    }
    let reserved: Vec<_> = (0..64).map(|index| [28, 56, 84, 112][index % 4]).collect();
    let fields = [
        [34, 200, 430, 156],
        [496, 228, 430, 128],
        [34, 532, 430, 100],
        [496, 560, 430, 72],
    ];
    let touches = [
        [34.0, 204.0, 249.0, 356.0, 249.0, 204.0, 464.0, 356.0],
        [496.0, 232.0, 711.0, 356.0, 711.0, 232.0, 926.0, 356.0],
        [34.0, 536.0, 249.0, 632.0, 249.0, 536.0, 464.0, 632.0],
        [496.0, 564.0, 711.0, 632.0, 711.0, 564.0, 926.0, 632.0],
    ];
    let frames = [BgaFrame::default(); 4];
    let mut scene = Scene::new(960, 720);
    for page in 0..16 {
        scene.clear();
        organisms::local_player_views_with_reserved_comparison_space(
            &mut scene,
            &preview,
            4_000_000_000,
            page,
            true,
            &frames,
            &reserved,
        )
        .unwrap();
        let prior: Vec<_> = scene
            .playfields()
            .iter()
            .map(|frame| (Arc::clone(&frame.instances), frame.top, frame.bottom))
            .collect();
        let comparisons: Vec<_> = scene
            .rectangles()
            .iter()
            .filter(|r| r.bounds[1] >= 172.0 && r.bounds[1] < 200.0)
            .map(|r| (r.bounds, r.color, r.uv))
            .collect();
        assert_eq!(
            rows(&scene, TextureId::WHITE)
                .iter()
                .filter(|r| r.color == color(0x1f2d40))
                .count(),
            0
        );
        scene.clear();
        organisms::local_player_views_with_reserved_comparison_space(
            &mut scene,
            &views,
            4_000_000_000,
            page,
            true,
            &frames,
            &reserved,
        )
        .unwrap();
        assert_eq!(scene.playfields().len(), 4);
        assert_eq!(
            scene
                .rectangles()
                .iter()
                .filter(|r| r.bounds[1] >= 172.0 && r.bounds[1] < 200.0)
                .map(|r| (r.bounds, r.color, r.uv))
                .collect::<Vec<_>>(),
            comparisons
        );
        for slot in 0..4 {
            let index = page * 4 + slot;
            let percent = 20 + index;
            let prefix = if percent >= 80 { "READY" } else { "GAUGE" };
            let rgb = if percent >= 80 { 0x61d69a } else { 0x4f92db };
            let [x, y, width, _] = crate::playfield_layout::local_panel_bounds(4, slot).unwrap();
            label_at(
                &scene,
                &format!("{prefix} {percent}.00%"),
                x + width - 98,
                y + 58,
                rgb,
            );
            assert!(rows(&scene, TextureId::WHITE).iter().any(|r| r.bounds
                == [(x + width - 100) as f32, (y + 56) as f32, 90.0, 14.0]
                && r.color == color(0x1f2d40)));
            let frame = &scene.playfields()[slot];
            assert!(Arc::ptr_eq(&prior[slot].0, &frame.instances));
            assert_eq!((frame.top, frame.bottom), (prior[slot].1, prior[slot].2));
            assert_eq!(
                local_field_bounds_with_comparison_space(4, slot, reserved[index]).unwrap(),
                fields[slot]
            );
            assert_eq!(
                (frame.top, frame.bottom),
                (
                    (fields[slot][1] + 4) as f32,
                    (fields[slot][1] + fields[slot][3] - 9) as f32
                )
            );
            let bounds =
                local_touch_bounds_with_comparison_space(&chart.lanes, 4, slot, reserved[index])
                    .unwrap();
            assert_eq!(bounds, touches[slot]);
            let mut router = TouchInputSetup::new(
                &[
                    0x11,
                    1,
                    3,
                    0,
                    1,
                    0x5754_4f55,
                    0,
                    0x12,
                    1,
                    3,
                    0,
                    1,
                    0x5754_4f55,
                    0,
                ],
                &bounds,
                &chart.lanes,
                4,
            )
            .unwrap();
            let original = touch();
            let routed = router
                .router
                .route_at(
                    &original,
                    Position2 {
                        x: touches[slot][0] + 1.0,
                        y: touches[slot][1] + 1.0,
                    },
                )
                .unwrap();
            let TouchRoute::Bound(bound) = routed else {
                panic!("actual unchanged contact field")
            };
            assert_eq!(bound.game_control, GameControlId(0x11));
            assert_eq!(bound.physical, original);
        }
    }
    scene.clear();
    assert!(organisms::local_players(&mut scene, &players, 4_000_000_000, 16).is_err());
    assert!(scene.rectangles().is_empty());
    for (before, after) in retained.iter().zip(&players) {
        assert_eq!(before.gauge, after.gauge);
        assert_eq!(before.score, after.score);
        assert_eq!(before.song_time, after.song_time);
        assert_eq!(before.competition, after.competition);
        assert_eq!(before.pressed_lanes, after.pressed_lanes);
        assert!(Arc::ptr_eq(
            before.chart.as_ref().unwrap(),
            after.chart.as_ref().unwrap()
        ));
    }
    let mut solo = Scene::new(960, 720);
    organisms::playfield(&mut solo, &chart, Timestamp::ZERO, 4_000_000_000).unwrap();
    let original = Arc::clone(&solo.playfields()[0].instances);
    let touch_before = default_touch_bounds(&chart.lanes).unwrap();
    gauge_hud(
        &mut solo,
        &players[0].gauge,
        Bounds {
            x: 750,
            y: 110,
            width: 186,
            height: 18,
        },
    )
    .unwrap();
    assert!(Arc::ptr_eq(&original, &solo.playfields()[0].instances));
    assert_eq!(
        touch_before,
        [80.0, 110.0, 400.0, 634.0, 400.0, 110.0, 720.0, 634.0]
    );
    assert_eq!(default_touch_bounds(&chart.lanes).unwrap(), touch_before);
    label_at(&solo, "GAUGE 20.00%", 752, 112, 0x4f92db);
}
