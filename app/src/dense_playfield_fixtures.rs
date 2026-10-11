//! Independent complete CPU presentation oracles; these do not execute a GPU.
use std::sync::Arc;

use beatkernel::{chart::ObjectId, time::Timestamp};
use beatkernel_bms::{parse, MineDamage};

use crate::{
    note_progress::{NoteProgress, NoteProgressPageUpdate, NoteState},
    player_chart::{PlayerChart, PlayerMine},
    playfield_gpu::{NoteInstance, PlayfieldCache},
    scene::Scene,
    ui::interaction::Bounds,
};

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}

fn bounds() -> Bounds {
    Bounds {
        x: 80,
        y: 106,
        width: 640,
        height: 528,
    }
}

fn dense_chart() -> PlayerChart {
    // Genuine LNTYPE1 pairs, taps and mines occupy three distinct lanes in one
    // four-second measure. Literal counts independently pin the old ceilings.
    let text = format!(
        "#BPM 60\n#WAV01 tone\n#LNTYPE 1\n#00051:{}\n#00012:{}\n#000D3:{}",
        "0101".repeat(2304),
        "01".repeat(2304),
        "1E".repeat(2304)
    );
    let source = parse(&text, Default::default()).unwrap();
    let chart = PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap();
    assert_eq!(chart.lanes, [0x11, 0x12, 0x13]);
    assert_eq!(chart.notes.len(), 4608);
    assert_eq!(chart.notes.iter().filter(|n| n.end.is_some()).count(), 2304);
    assert_eq!(chart.mines().len(), 2304);
    chart
}

fn color(kind: f32, rgb: [u8; 3]) -> [f32; 4] {
    [
        kind,
        f32::from(rgb[0]) / 255.,
        f32::from(rgb[1]) / 255.,
        f32::from(rgb[2]) / 255.,
    ]
}

fn close(actual: f32, expected: f64) {
    assert!(actual.is_finite());
    assert!(
        (f64::from(actual) - expected).abs() <= 0.001,
        "{actual} != {expected}"
    );
}

fn assert_note(
    instance: &NoteInstance,
    lane: usize,
    start: i64,
    end: i64,
    kind: f32,
    now: i64,
    field: Bounds,
) {
    let left = field.x + (lane as i64 * field.width / 3);
    let right = field.x + ((lane as i64 + 1) * field.width / 3);
    let inset = if kind == 0. { 6 } else { 3 };
    assert_eq!(instance.geometry[0], (left + inset) as f32);
    assert_eq!(
        instance.geometry[1],
        (right - left - inset * 2).max(1) as f32
    );
    let line = field.y + field.height - 24;
    let span = field.height - 28;
    for (actual, time) in [(instance.geometry[2], start), (instance.geometry[3], end)] {
        let delta = i128::from(time) - i128::from(now);
        close(
            actual,
            line as f64 - delta as f64 * span as f64 / 4_000_000_000.,
        );
    }
    let rgb = match kind as u8 {
        0 => [53, 126, 152],
        1 => [135, 231, 255],
        _ => [116, 229, 197],
    };
    assert_eq!(instance.appearance, color(kind, rgb));
}

#[test]
fn dense_playfield_real_bms_has_complete_ordered_membership_and_11520_primitives() {
    let chart = dense_chart();
    let mut notes = Vec::new();
    let mut mines = Vec::new();
    chart
        .visible_note_indices_checked(ts(0), 4_000_000_000, 0, &mut notes)
        .unwrap();
    chart
        .visible_mine_indices_checked(ts(0), 4_000_000_000, 0, &mut mines)
        .unwrap();
    assert_eq!(notes, (0..4608).collect::<Vec<_>>());
    assert_eq!(mines, (0..2304).collect::<Vec<_>>());
    let mut cache = PlayfieldCache::default();
    let frame = cache
        .frame_indexed_with_mines_and_progress(
            &chart.notes,
            &notes,
            chart.mines(),
            &mines,
            3,
            bounds(),
            ts(0),
            4_000_000_000,
            None,
        )
        .unwrap();
    assert_eq!(frame.instances.len(), 11520);
    let mut cursor = 0;
    for note in &chart.notes {
        let kinds: &[f32] = if note.end.is_some() {
            &[0., 1., 2.]
        } else {
            &[2.]
        };
        for &kind in kinds {
            assert_note(
                &frame.instances[cursor],
                note.lane_index,
                note.start.as_nanos(),
                note.end.unwrap_or(note.start).as_nanos(),
                kind,
                0,
                bounds(),
            );
            cursor += 1;
        }
    }
    assert_eq!(cursor, 9216);
    for (instance, mine) in frame.instances[cursor..].iter().zip(chart.mines()) {
        assert_eq!(instance.geometry[0..2], [509., 208.]);
        let y = 610. - mine.at.as_nanos() as f64 / 8_000_000.;
        close(instance.geometry[2], y);
        close(instance.geometry[3], y);
        assert_eq!(instance.appearance, color(2., [239, 99, 114]));
    }
    assert_eq!((frame.top, frame.bottom, frame.drift), (110., 625., 0.));
    let mut scene = Scene::new(960, 720);
    scene
        .playfield(&chart, ts(0), 4_000_000_000, bounds())
        .unwrap();
    assert_eq!(scene.playfields()[0].instances.len(), 11520);
    for (actual, expected) in scene.playfields()[0]
        .instances
        .iter()
        .zip(frame.instances.iter())
    {
        assert_eq!(actual.geometry, expected.geometry);
        assert_eq!(actual.appearance, expected.appearance);
    }
    assert!(scene.status().is_ok());
}

fn progress(chart: Arc<PlayerChart>, complete_holds: bool) -> NoteProgress {
    let mut progress = NoteProgress::new(Arc::clone(&chart)).unwrap();
    let packed: Vec<_> = chart
        .notes
        .chunks(4096)
        .map(|notes| {
            let mut bits = [0u64; 128];
            for (offset, note) in notes.iter().enumerate() {
                let state = if note.end.is_some() && !complete_holds {
                    1
                } else {
                    2
                };
                bits[offset / 32] |= state << ((offset % 32) * 2);
            }
            bits
        })
        .collect();
    let updates: Vec<_> = chart
        .notes
        .chunks(4096)
        .enumerate()
        .map(|(index, notes)| NoteProgressPageUpdate {
            index,
            valid_count: notes.len(),
            completed_count: notes
                .iter()
                .filter(|n| complete_holds || n.end.is_none())
                .count(),
            packed_states: &packed[index],
        })
        .collect();
    progress.apply_page_updates(&updates, None).unwrap();
    progress
}

#[test]
fn dense_playfield_holding_and_completed_progress_preserve_all_mines() {
    let chart = Arc::new(dense_chart());
    let holding = progress(Arc::clone(&chart), false);
    let completed = progress(Arc::clone(&chart), true);
    let all_notes: Vec<_> = (0..4608).collect();
    let all_mines: Vec<_> = (0..2304).collect();
    let mut cache = PlayfieldCache::default();
    let held = cache
        .frame_indexed_with_mines_and_progress(
            &chart.notes,
            &all_notes,
            chart.mines(),
            &all_mines,
            3,
            bounds(),
            ts(0),
            4_000_000_000,
            Some(&holding),
        )
        .unwrap();
    assert_eq!(held.instances.len(), 6912);
    for (pair, note) in held.instances[..4608]
        .chunks_exact(2)
        .zip(chart.notes.iter().filter(|n| n.end.is_some()))
    {
        assert_eq!(
            holding.state(chart.note_index_by_object(note.object).unwrap()),
            Some(NoteState::Holding)
        );
        assert_note(
            &pair[0],
            0,
            note.start.as_nanos(),
            note.end.unwrap().as_nanos(),
            0.,
            0,
            bounds(),
        );
        assert_note(
            &pair[1],
            0,
            note.start.as_nanos(),
            note.end.unwrap().as_nanos(),
            1.,
            0,
            bounds(),
        );
    }
    let done = cache
        .frame_indexed_with_mines_and_progress(
            &chart.notes,
            &all_notes,
            chart.mines(),
            &all_mines,
            3,
            bounds(),
            ts(0),
            4_000_000_000,
            Some(&completed),
        )
        .unwrap();
    assert_eq!(done.instances.len(), 2304);
    for (actual, expected) in done.instances.iter().zip(&held.instances[4608..]) {
        assert_eq!(actual.geometry, expected.geometry);
        assert_eq!(actual.appearance, expected.appearance);
    }
    let mut scene = Scene::new(960, 720);
    scene
        .playfield_with_progress(&chart, ts(0), 4_000_000_000, bounds(), Some(&holding))
        .unwrap();
    assert_eq!(scene.playfields()[0].instances.len(), 6912);
    scene.clear();
    scene
        .playfield_with_progress(&chart, ts(0), 4_000_000_000, bounds(), Some(&completed))
        .unwrap();
    assert_eq!(scene.playfields()[0].instances.len(), 2304);
}

#[test]
fn dense_playfield_cache_warms_rebases_seeks_resizes_and_preserves_old_packet_on_refusal() {
    let chart = dense_chart();
    let notes: Vec<_> = (0..4608).collect();
    let mines: Vec<_> = (0..2304).collect();
    let mut cache = PlayfieldCache::default();
    let mut get = |at, field| {
        cache
            .frame_indexed_with_mines_and_progress(
                &chart.notes,
                &notes,
                chart.mines(),
                &mines,
                3,
                field,
                ts(at),
                4_000_000_000,
                None,
            )
            .unwrap()
    };
    let first = get(0, bounds());
    let warm = get(1_000_000_000, bounds());
    assert!(Arc::ptr_eq(&first.instances, &warm.instances));
    assert_eq!(warm.drift, 125.);
    let rebase = get(1_000_000_001, bounds());
    assert!(!Arc::ptr_eq(&warm.instances, &rebase.instances));
    assert_eq!(rebase.drift, 0.);
    let seek = get(0, bounds());
    assert!(!Arc::ptr_eq(&rebase.instances, &seek.instances));
    let resized = get(
        0,
        Bounds {
            width: 320,
            ..bounds()
        },
    );
    assert!(!Arc::ptr_eq(&seek.instances, &resized.instances));
    assert_eq!(resized.instances.len(), 11520);
    drop(get);
    for (note_indices, mine_indices) in [(&[4608][..], &mines[..]), (&notes[..], &[2304][..])] {
        assert!(cache
            .frame_indexed_with_mines_and_progress(
                &chart.notes,
                note_indices,
                chart.mines(),
                mine_indices,
                3,
                bounds(),
                ts(0),
                4_000_000_000,
                None
            )
            .is_err());
        let recovered = cache
            .frame_indexed_with_mines_and_progress(
                &chart.notes,
                &notes,
                chart.mines(),
                &mines,
                3,
                Bounds {
                    width: 320,
                    ..bounds()
                },
                ts(0),
                4_000_000_000,
                None,
            )
            .unwrap();
        assert!(Arc::ptr_eq(&resized.instances, &recovered.instances));
    }
}

#[test]
fn dense_playfield_fourth_failure_is_sticky_and_clear_recovers_same_caches() {
    let chart = dense_chart();
    let mut scene = Scene::new(960, 720);
    for _ in 0..4 {
        scene
            .playfield(&chart, ts(0), 4_000_000_000, bounds())
            .unwrap();
    }
    let prior: Vec<_> = scene
        .playfields()
        .iter()
        .map(|f| Arc::clone(&f.instances))
        .collect();
    scene.clear();
    for _ in 0..3 {
        scene
            .playfield(&chart, ts(0), 4_000_000_000, bounds())
            .unwrap();
    }
    let batches = scene.batches().len();
    let mut invalid = chart.clone();
    invalid.lanes.clear();
    let error = scene
        .playfield(&invalid, ts(0), 4_000_000_000, bounds())
        .unwrap_err();
    assert_eq!(scene.status(), Err(error.clone()));
    assert_eq!(scene.playfields().len(), 3);
    assert_eq!(scene.batches().len(), batches);
    assert!(scene
        .playfield(&chart, ts(0), 4_000_000_000, bounds())
        .is_err());
    assert_eq!(scene.playfields().len(), 3);
    assert_eq!(scene.status(), Err(error));
    scene.clear();
    for _ in 0..4 {
        scene
            .playfield(&chart, ts(0), 4_000_000_000, bounds())
            .unwrap();
    }
    assert!(scene.status().is_ok());
    for (old, new) in prior.iter().zip(scene.playfields()) {
        assert!(Arc::ptr_eq(old, &new.instances));
    }
    assert!(scene
        .playfield(&chart, ts(0), 4_000_000_000, bounds())
        .is_err());
    assert!(scene.status().is_err());
    assert_eq!(scene.playfields().len(), 4);
}

#[test]
fn dense_playfield_invalid_prepared_index_extent_rejects_without_publishing() {
    for expand in [false, true] {
        let mut chart = dense_chart();
        if expand {
            let mut note = chart.notes.last().unwrap().clone();
            note.object = ObjectId(99_999);
            chart.notes.push(note);
        } else {
            chart.notes.pop();
        }
        let mut scratch = vec![usize::MAX];
        assert!(chart
            .visible_note_indices_checked(ts(0), 4_000_000_000, 0, &mut scratch)
            .is_err());
        assert!(scratch.is_empty());
        let mut scene = Scene::new(960, 720);
        assert!(scene
            .playfield(&chart, ts(0), 4_000_000_000, bounds())
            .is_err());
        assert!(scene.playfields().is_empty());
        assert!(scene.batches().is_empty());
        assert!(scene.status().is_err());
    }
}

#[test]
fn dense_playfield_combined_source_refusal_preserves_warmed_cache() {
    let chart = dense_chart();
    let mut cache = PlayfieldCache::default();
    let first = cache
        .frame_indexed_with_progress(&chart.notes, &[0], 3, bounds(), ts(0), 4_000_000_000, None)
        .unwrap();
    let mines = vec![
        PlayerMine {
            ordinal: 0,
            lane_index: 0,
            at: ts(0),
            damage: MineDamage::from_raw(1).unwrap()
        };
        beatkernel::chart::MAX_SOURCE_ITEMS
    ];
    assert!(cache
        .frame_indexed_with_mines_and_progress(
            &chart.notes,
            &[0],
            &mines,
            &[],
            3,
            bounds(),
            ts(0),
            4_000_000_000,
            None
        )
        .is_err());
    let recovered = cache
        .frame_indexed_with_progress(&chart.notes, &[0], 3, bounds(), ts(0), 4_000_000_000, None)
        .unwrap();
    assert!(Arc::ptr_eq(&first.instances, &recovered.instances));
}

#[test]
fn dense_playfield_all_64_roster_pages_keep_four_complete_retained_fields() {
    use crate::{local_players::PlayerId, player::LocalPlayerSnapshot, ui::organisms};
    let chart = Arc::new(dense_chart());
    let pending = NoteProgress::new(Arc::clone(&chart)).unwrap();
    let completed = progress(Arc::clone(&chart), true);
    let players: Vec<_> = (0..64)
        .map(|index| LocalPlayerSnapshot {
            bms_score: None,
            player: PlayerId(index + 1),
            chart: Some(Arc::clone(&chart)),
            song_time: Some(ts(0)),
            score: Default::default(),
            mine_damage: Default::default(),
            gauge: Default::default(),
            last_judge: None,
            recent_results: vec![],
            pressed_lanes: 0,
            note_progress: Some(if index % 2 == 0 {
                completed.clone()
            } else {
                pending.clone()
            }),
            competition: None,
        })
        .collect();
    let mut scene = Scene::new(960, 720);
    for page in 0..16 {
        scene.clear();
        organisms::local_players(&mut scene, &players, 4_000_000_000, page).unwrap();
        assert!(scene.status().is_ok());
        assert_eq!(scene.playfields().len(), 4);
        assert_eq!(
            scene
                .batches()
                .iter()
                .filter_map(|b| b.playfield)
                .collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        let prior: Vec<_> = scene
            .playfields()
            .iter()
            .map(|f| Arc::clone(&f.instances))
            .collect();
        for (slot, frame) in scene.playfields().iter().enumerate() {
            assert_eq!(
                frame.instances.len(),
                if slot % 2 == 0 { 2304 } else { 11520 }
            );
            assert!(frame.instances[frame.instances.len() - 2304..]
                .iter()
                .all(|m| m.appearance == color(2., [239, 99, 114])));
            let [x, _, width, _] = crate::playfield_layout::local_field_bounds(4, slot).unwrap();
            for mine in &frame.instances[frame.instances.len() - 2304..] {
                assert!(mine.geometry[0] >= x as f32);
                assert!(mine.geometry[0] + mine.geometry[1] <= (x + width) as f32);
            }
        }
        scene.clear();
        organisms::local_players(&mut scene, &players, 4_000_000_000, page).unwrap();
        for (old, frame) in prior.iter().zip(scene.playfields()) {
            assert!(Arc::ptr_eq(old, &frame.instances));
        }
    }
    scene.clear();
    assert!(organisms::local_players(&mut scene, &players, 4_000_000_000, 16).is_err());
    assert!(scene.playfields().is_empty());
}

fn compose_dense_public(
    scene: &mut Scene,
    chart: &PlayerChart,
    progress: &NoteProgress,
) -> Result<(), String> {
    crate::ui::organisms::playfield_in_with_progress(
        scene,
        chart,
        ts(0),
        4_000_000_000,
        bounds(),
        &[],
        0,
        Some(progress),
    )
}

#[test]
fn dense_playfield_public_foreign_progress_fourth_field_poison_is_sticky_until_clear() {
    use crate::{bga_render::BgaFrame, ui::organisms};
    let chart = Arc::new(dense_chart());
    let pending = NoteProgress::new(Arc::clone(&chart)).unwrap();
    // Equal chart values do not authorize progress from another preparation.
    let foreign_chart = Arc::new((*chart).clone());
    assert!(!Arc::ptr_eq(&chart, &foreign_chart));
    assert_eq!(chart.notes, foreign_chart.notes);
    assert_eq!(chart.mines(), foreign_chart.mines());
    let foreign = NoteProgress::new(foreign_chart).unwrap();
    let mut scene = Scene::new(960, 720);
    for _ in 0..4 {
        compose_dense_public(&mut scene, &chart, &pending).unwrap();
    }
    let warmed: Vec<_> = scene
        .playfields()
        .iter()
        .map(|f| Arc::clone(&f.instances))
        .collect();
    scene.clear();
    for _ in 0..3 {
        compose_dense_public(&mut scene, &chart, &pending).unwrap();
    }
    let rectangles = scene.rectangles().len();
    let batches = scene.batches().len();
    let epoch = scene.geometry_stamp().1;
    let error = organisms::playfield_in_with_background(
        &mut scene,
        &chart,
        ts(0),
        4_000_000_000,
        bounds(),
        &[],
        0,
        Some(&foreign),
        BgaFrame::default(),
    )
    .unwrap_err();
    assert_eq!(error, "note progress belongs to another prepared chart");
    assert_eq!(scene.status(), Err(error.clone()));
    assert_eq!(
        compose_dense_public(&mut scene, &chart, &pending),
        Err(error.clone())
    );
    // An unrelated, earlier background validation must not replace the first
    // error or make the existing three-field prefix a successful candidate.
    assert_eq!(
        organisms::playfield_in_with_background(
            &mut scene,
            &chart,
            ts(0),
            4_000_000_000,
            bounds(),
            &[],
            0,
            Some(&pending),
            BgaFrame {
                active: true,
                unavailable: 5,
                ..Default::default()
            },
        ),
        Err(error.clone())
    );
    assert_eq!(scene.status(), Err(error));
    assert_eq!(scene.playfields().len(), 3);
    assert_eq!(scene.rectangles().len(), rectangles);
    assert_eq!(scene.batches().len(), batches);
    assert_eq!(scene.geometry_stamp().1, epoch);
    for (old, current) in warmed.iter().zip(scene.playfields()) {
        assert!(Arc::ptr_eq(old, &current.instances));
        assert_eq!(current.instances.len(), 11520);
    }
    scene.clear();
    for _ in 0..4 {
        compose_dense_public(&mut scene, &chart, &pending).unwrap();
    }
    assert!(scene.status().is_ok());
    assert_eq!(scene.playfields().len(), 4);
    for (old, current) in warmed.iter().zip(scene.playfields()) {
        assert!(Arc::ptr_eq(old, &current.instances));
        assert_eq!(current.instances.len(), 11520);
    }
}

#[test]
fn dense_playfield_public_early_input_refusals_poison_good_prefix_and_recover_retained_fields() {
    use crate::{
        bga_render::{BgaFrame, BgaSprite},
        texture::TextureId,
        ui::organisms,
    };
    use beatkernel::judge::{JudgeEvent, JudgeGrade, JudgeOutcome, JudgeStage};
    let chart = Arc::new(dense_chart());
    let pending = NoteProgress::new(Arc::clone(&chart)).unwrap();
    let tap = chart.notes.iter().find(|note| note.end.is_none()).unwrap();
    let event = JudgeEvent {
        object: tap.object,
        at: ts(0),
        stage: JudgeStage::Instant,
        input: None,
        outcome: JudgeOutcome::Hit {
            grade: JudgeGrade(1),
            delta: beatkernel::time::Duration::ZERO,
        },
    };
    let cases = [
        (
            "BGA selection",
            4_000_000_000,
            bounds(),
            vec![],
            0,
            BgaFrame {
                active: true,
                unavailable: 5,
                ..Default::default()
            },
            "invalid BGA frame selection",
        ),
        (
            "BGA sprite",
            4_000_000_000,
            bounds(),
            vec![],
            0,
            BgaFrame {
                active: true,
                base: Some(BgaSprite {
                    texture: TextureId::FONT,
                    width: 0,
                    height: 1,
                }),
                ..Default::default()
            },
            "invalid BGA sprite extent",
        ),
        (
            "pressed bit 18",
            4_000_000_000,
            bounds(),
            vec![],
            1 << 18,
            BgaFrame::default(),
            "pressed lane mask exceeds eighteen lanes",
        ),
        (
            "pressed bit 31",
            4_000_000_000,
            bounds(),
            vec![],
            1 << 31,
            BgaFrame::default(),
            "pressed lane mask exceeds eighteen lanes",
        ),
        (
            "129 feedback events",
            4_000_000_000,
            bounds(),
            vec![event; 129],
            0,
            BgaFrame::default(),
            "judge feedback exceeds result or lane capacity",
        ),
        (
            "zero horizon",
            0,
            bounds(),
            vec![],
            0,
            BgaFrame::default(),
            "playfield lookahead must be positive",
        ),
        (
            "negative horizon",
            -1,
            bounds(),
            vec![],
            0,
            BgaFrame::default(),
            "playfield lookahead must be positive",
        ),
        (
            "negative x",
            4_000_000_000,
            Bounds { x: -1, ..bounds() },
            vec![],
            0,
            BgaFrame::default(),
            "playfield bounds do not fit viewport or lane geometry",
        ),
        (
            "zero width",
            4_000_000_000,
            Bounds {
                width: 0,
                ..bounds()
            },
            vec![],
            0,
            BgaFrame::default(),
            "playfield bounds do not fit viewport or lane geometry",
        ),
        (
            "height 31",
            4_000_000_000,
            Bounds {
                height: 31,
                ..bounds()
            },
            vec![],
            0,
            BgaFrame::default(),
            "playfield bounds do not fit viewport or lane geometry",
        ),
        (
            "viewport overflow",
            4_000_000_000,
            Bounds {
                width: 881,
                ..bounds()
            },
            vec![],
            0,
            BgaFrame::default(),
            "playfield bounds do not fit viewport or lane geometry",
        ),
        (
            "three lanes need 39 pixels",
            4_000_000_000,
            Bounds {
                width: 38,
                ..bounds()
            },
            vec![],
            0,
            BgaFrame::default(),
            "playfield bounds do not fit viewport or lane geometry",
        ),
    ];
    let mut scene = Scene::new(960, 720);
    for _ in 0..4 {
        compose_dense_public(&mut scene, &chart, &pending).unwrap();
    }
    let warmed: Vec<_> = scene
        .playfields()
        .iter()
        .map(|f| Arc::clone(&f.instances))
        .collect();
    for (label, horizon, field, recent, mask, frame, expected) in cases {
        scene.clear();
        for _ in 0..3 {
            compose_dense_public(&mut scene, &chart, &pending).unwrap();
        }
        let rectangles = scene.rectangles().len();
        let batches = scene.batches().len();
        let epoch = scene.geometry_stamp().1;
        let error = organisms::playfield_in_with_background(
            &mut scene,
            &chart,
            ts(0),
            horizon,
            field,
            &recent,
            mask,
            Some(&pending),
            frame,
        )
        .unwrap_err();
        assert_eq!(error, expected, "{label}");
        assert_eq!(scene.status(), Err(error.clone()), "{label}");
        assert_eq!(
            compose_dense_public(&mut scene, &chart, &pending),
            Err(error.clone()),
            "{label}"
        );
        // This simultaneously supplies other invalid reasons. Once poisoned,
        // the first refusal remains the observable result of every attempt.
        assert_eq!(
            organisms::playfield_in_with_background(
                &mut scene,
                &chart,
                ts(0),
                -1,
                Bounds { x: -1, ..bounds() },
                &[],
                1 << 31,
                Some(&pending),
                BgaFrame {
                    active: true,
                    unavailable: 5,
                    ..Default::default()
                }
            ),
            Err(error.clone()),
            "{label}"
        );
        assert_eq!(scene.status(), Err(error), "{label}");
        assert_eq!(scene.playfields().len(), 3, "{label}");
        assert_eq!(scene.rectangles().len(), rectangles, "{label}");
        assert_eq!(scene.batches().len(), batches, "{label}");
        assert_eq!(scene.geometry_stamp().1, epoch, "{label}");
        for (old, current) in warmed.iter().zip(scene.playfields()) {
            assert!(Arc::ptr_eq(old, &current.instances), "{label}");
            assert_eq!(current.instances.len(), 11520, "{label}");
        }
        scene.clear();
        for _ in 0..4 {
            compose_dense_public(&mut scene, &chart, &pending).unwrap();
        }
        assert!(scene.status().is_ok(), "{label}");
        assert_eq!(scene.playfields().len(), 4, "{label}");
        for (old, current) in warmed.iter().zip(scene.playfields()) {
            assert!(Arc::ptr_eq(old, &current.instances), "{label}");
            assert_eq!(current.instances.len(), 11520, "{label}");
        }
    }
}

#[test]
fn dense_playfield_public_local_roster_bridges_poison_prefix_and_clear_recovers() {
    use crate::{
        bga_render::BgaFrame,
        local_players::PlayerId,
        player::LocalPlayerSnapshot,
        ui::organisms::{self, LocalPlayerView, VisualLocalPlayerView},
    };
    use beatkernel::judge::{JudgeEvent, JudgeGrade, JudgeOutcome, JudgeStage};

    let chart = Arc::new(dense_chart());
    let pending = NoteProgress::new(Arc::clone(&chart)).unwrap();
    let foreign = NoteProgress::new(Arc::new((*chart).clone())).unwrap();
    let snapshot = |id| LocalPlayerSnapshot {
        player: PlayerId(id),
        chart: Some(Arc::clone(&chart)),
        song_time: Some(ts(0)),
        score: Default::default(),
        bms_score: None,
        mine_damage: Default::default(),
        gauge: Default::default(),
        last_judge: None,
        recent_results: vec![],
        pressed_lanes: 0,
        note_progress: Some(pending.clone()),
        competition: None,
    };
    let players: Vec<_> = (1..=4).map(&snapshot).collect();
    let over_roster: Vec<_> = (1..=65).map(&snapshot).collect();
    let views: Vec<_> = players.iter().map(LocalPlayerView::from).collect();
    let visuals: Vec<_> = views
        .iter()
        .copied()
        .map(VisualLocalPlayerView::from)
        .collect();
    let event = JudgeEvent {
        object: chart.notes.iter().find(|n| n.end.is_none()).unwrap().object,
        at: ts(0),
        stage: JudgeStage::Instant,
        input: None,
        outcome: JudgeOutcome::Hit {
            grade: JudgeGrade(1),
            delta: beatkernel::time::Duration::ZERO,
        },
    };
    let excessive_feedback = [event; 129];
    let frames = [BgaFrame::default(); 4];
    let mut scene = Scene::new(960, 720);
    for _ in 0..4 {
        compose_dense_public(&mut scene, &chart, &pending).unwrap();
    }
    let warmed: Vec<_> = scene
        .playfields()
        .iter()
        .map(|f| Arc::clone(&f.instances))
        .collect();
    // The first three cases exercise snapshot/borrowed bridges whose own early
    // checks must participate in the same failure boundary as visual composition.
    for case in [
        "snapshot empty roster",
        "snapshot 65 roster",
        "snapshot page",
        "borrowed page",
        "borrowed reservations shape",
        "visual page",
        "visual reservations shape",
        "fourth foreign progress",
        "fourth bad BGA",
        "fourth mask",
        "fourth feedback",
        "zero lookahead",
        "zero identity",
        "duplicate identity",
        "negative reserved layout",
        "oversize reserved layout",
    ] {
        scene.clear();
        for _ in 0..3 {
            compose_dense_public(&mut scene, &chart, &pending).unwrap();
        }
        let rectangles = scene.rectangles().len();
        let batches = scene.batches().len();
        let epoch = scene.geometry_stamp().1;
        let mut bad_views = visuals.clone();
        let mut bad_frames = frames;
        let mut horizon = 4_000_000_000;
        let mut page = 0;
        let mut reserved = None;
        let negative_spaces = [0, 0, 0, -1];
        let oversized_spaces = [0, 0, 0, 141];
        let short_spaces = [0; 3];
        let result = match case {
            "snapshot empty roster" => organisms::local_players_with_background(
                &mut scene,
                &[],
                horizon,
                0,
                false,
                &frames,
            ),
            "snapshot 65 roster" => organisms::local_players_with_background(
                &mut scene,
                &over_roster,
                horizon,
                0,
                false,
                &frames,
            ),
            "snapshot page" => organisms::local_players_with_background(
                &mut scene, &players, horizon, 1, false, &frames,
            ),
            "borrowed page" => organisms::local_player_views_with_background(
                &mut scene,
                &views,
                horizon,
                usize::MAX,
                false,
                &frames,
            ),
            "borrowed reservations shape" => {
                organisms::local_player_views_with_reserved_comparison_space(
                    &mut scene,
                    &views,
                    horizon,
                    0,
                    false,
                    &frames,
                    &short_spaces,
                )
            }
            _ => {
                match case {
                    "visual page" => page = 1,
                    "visual reservations shape" => reserved = Some(short_spaces.as_slice()),
                    "fourth foreign progress" => bad_views[3].note_progress = Some(&foreign),
                    "fourth bad BGA" => {
                        bad_frames[3] = BgaFrame {
                            active: true,
                            unavailable: 5,
                            ..Default::default()
                        }
                    }
                    "fourth mask" => bad_views[3].pressed_lanes = 1 << 31,
                    "fourth feedback" => bad_views[3].recent_results = &excessive_feedback,
                    "zero lookahead" => horizon = 0,
                    "zero identity" => bad_views[3].player = PlayerId(0),
                    "duplicate identity" => bad_views[3].player = bad_views[0].player,
                    "negative reserved layout" => reserved = Some(negative_spaces.as_slice()),
                    "oversize reserved layout" => reserved = Some(oversized_spaces.as_slice()),
                    _ => unreachable!(),
                }
                organisms::visual_local_player_views(
                    &mut scene,
                    &bad_views,
                    horizon,
                    page,
                    false,
                    &bad_frames,
                    reserved,
                )
            }
        };
        let error = result.expect_err(case);
        assert!(!error.is_empty(), "{case}");
        assert_eq!(scene.status(), Err(error.clone()), "{case}");
        assert_eq!(
            organisms::local_players_with_background(
                &mut scene,
                &players,
                4_000_000_000,
                0,
                false,
                &frames
            ),
            Err(error.clone()),
            "{case}"
        );
        assert_eq!(
            organisms::local_player_views_with_background(
                &mut scene,
                &views,
                4_000_000_000,
                0,
                false,
                &frames
            ),
            Err(error.clone()),
            "{case}"
        );
        assert_eq!(
            organisms::local_player_views_with_reserved_comparison_space(
                &mut scene,
                &views,
                4_000_000_000,
                0,
                false,
                &frames,
                &short_spaces
            ),
            Err(error.clone()),
            "{case}"
        );
        assert_eq!(
            organisms::visual_local_player_views(
                &mut scene, &visuals, -1, 99, false, &frames, None
            ),
            Err(error.clone()),
            "{case}"
        );
        assert_eq!(scene.status(), Err(error), "{case}");
        assert_eq!(scene.playfields().len(), 3, "{case}");
        assert_eq!(scene.rectangles().len(), rectangles, "{case}");
        assert_eq!(scene.batches().len(), batches, "{case}");
        assert_eq!(scene.geometry_stamp().1, epoch, "{case}");
        for (old, frame) in warmed.iter().zip(scene.playfields()) {
            assert!(Arc::ptr_eq(old, &frame.instances), "{case}");
            assert_eq!(frame.instances.len(), 11520, "{case}");
        }
        scene.clear();
        for _ in 0..4 {
            compose_dense_public(&mut scene, &chart, &pending).unwrap();
        }
        assert!(scene.status().is_ok(), "{case}");
        assert_eq!(scene.playfields().len(), 4, "{case}");
        for (old, frame) in warmed.iter().zip(scene.playfields()) {
            assert!(Arc::ptr_eq(old, &frame.instances), "{case}");
        }
    }
    // Both snapshot and borrowed bridges recover to their complete local layout,
    // retaining identical geometry allocations when the layout does not change.
    scene.clear();
    organisms::local_players_with_background(
        &mut scene,
        &players,
        4_000_000_000,
        0,
        false,
        &frames,
    )
    .unwrap();
    let local_warmed: Vec<_> = scene
        .playfields()
        .iter()
        .map(|f| Arc::clone(&f.instances))
        .collect();
    scene.clear();
    organisms::local_player_views_with_background(
        &mut scene,
        &views,
        4_000_000_000,
        0,
        false,
        &frames,
    )
    .unwrap();
    assert!(scene.status().is_ok());
    assert_eq!(scene.playfields().len(), 4);
    for (old, frame) in local_warmed.iter().zip(scene.playfields()) {
        assert!(Arc::ptr_eq(old, &frame.instances));
        assert_eq!(frame.instances.len(), 11520);
    }
}

#[test]
fn dense_playfield_cold_browser_bridge_unavailable_scalars_poison_prefix_until_clear() {
    use crate::{
        bga_render::BgaFrame,
        browser_render_state::{
            BrowserRenderState, RenderFrame, RenderMemberScalars, RenderMemberUpdate,
        },
        image_assets::ImageAssets,
        local_players::PlayerId,
        ui::organisms,
    };
    let chart = Arc::new(dense_chart());
    let pending = NoteProgress::new(Arc::clone(&chart)).unwrap();
    let roster = vec![PlayerId(1), PlayerId(2), PlayerId(3), PlayerId(4)];
    let mut state = BrowserRenderState::import_visual(
        7,
        19,
        chart.export_visual(),
        ImageAssets::default().export_visual(),
        roster.clone(),
    )
    .unwrap();
    assert!(state
        .members()
        .iter()
        .all(|member| member.scalars.is_none()));
    let frames = [BgaFrame::default(); 4];
    let mut scratch = std::array::from_fn(|_| Vec::new());
    let mut scene = Scene::new(960, 720);
    for _ in 0..3 {
        compose_dense_public(&mut scene, &chart, &pending).unwrap();
    }
    let prefix: Vec<_> = scene
        .playfields()
        .iter()
        .map(|f| Arc::clone(&f.instances))
        .collect();
    let rectangles = scene.rectangles().len();
    let batches = scene.batches().len();
    let epoch = scene.geometry_stamp().1;
    let error = organisms::visual_local_render_state(&mut scene, &state, &frames, &mut scratch)
        .unwrap_err();
    assert_eq!(error, "visual frame unavailable");
    assert_eq!(scene.status(), Err(error.clone()));
    state
        .apply_frame(&RenderFrame {
            generation: 7,
            content: 19,
            sequence: 1,
            page: 0,
            lookahead_ns: 4_000_000_000,
            members: roster
                .iter()
                .map(|id| {
                    RenderMemberUpdate::from_progress(
                        *id,
                        RenderMemberScalars::default(),
                        &pending,
                        &pending,
                    )
                    .unwrap()
                })
                .collect(),
            room: None,
            room_disabled: false,
        })
        .unwrap();
    assert!(state
        .members()
        .iter()
        .all(|member| member.scalars.is_some()));
    assert_eq!(
        organisms::visual_local_render_state(&mut scene, &state, &frames, &mut scratch),
        Err(error.clone())
    );
    assert_eq!(
        organisms::local_players_with_background(&mut scene, &[], 4_000_000_000, 0, false, &frames),
        Err(error.clone())
    );
    assert_eq!(scene.status(), Err(error));
    assert_eq!(scene.geometry_stamp().1, epoch);
    assert_eq!(scene.rectangles().len(), rectangles);
    assert_eq!(scene.batches().len(), batches);
    assert_eq!(scene.playfields().len(), 3);
    for (old, current) in prefix.iter().zip(scene.playfields()) {
        assert!(Arc::ptr_eq(old, &current.instances));
        assert_eq!(current.instances.len(), 11520);
    }
    scene.clear();
    organisms::visual_local_render_state(&mut scene, &state, &frames, &mut scratch).unwrap();
    assert!(scene.status().is_ok());
    assert_eq!(scene.playfields().len(), 4);
    let warmed: Vec<_> = scene
        .playfields()
        .iter()
        .map(|f| Arc::clone(&f.instances))
        .collect();
    scene.clear();
    organisms::visual_local_render_state(&mut scene, &state, &frames, &mut scratch).unwrap();
    assert!(scene.status().is_ok());
    assert_eq!(scene.playfields().len(), 4);
    for (old, current) in warmed.iter().zip(scene.playfields()) {
        assert!(Arc::ptr_eq(old, &current.instances));
        assert_eq!(current.instances.len(), 11520);
    }
}
