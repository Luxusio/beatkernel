//! Deferred retained-mine projection fixtures. These call portable preparation
//! and Scene composition; they do not execute a GPU or grant source admission.
use crate::{
    note_progress::NoteProgress,
    player_chart::{MAX_VISIBLE_MINES, PlayerChart, PlayerMine},
};
use beatkernel::{chart::ObjectId, time::Timestamp};
use beatkernel_bms::{BmsChart, MineDamage, parse};
use std::sync::Arc;

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn source(text: &str) -> BmsChart {
    parse(text, Default::default()).unwrap()
}
fn project(source: &BmsChart) -> PlayerChart {
    PlayerChart::from_compiled(source, &source.compile().unwrap().chart).unwrap()
}
fn model(text: &str) -> PlayerChart {
    project(&source(text))
}
#[cfg(feature = "graphics")]
fn mixed_source() -> BmsChart {
    source(
        "#BPM 120\n#WAV01 tone\n#LNTYPE 1\n#00051:0101\n#00012:00010000\n#000D1:001E000000000000\n#000D2:000000ZZ00000000",
    )
}

#[test]
fn prepared_mines_keep_full_identity_sorted_ties_and_never_become_note_progress() {
    let ordinary = source("#BPM 120\n#WAV01 tone\n#00011:01\n#00032:01");
    let mut augmented =
        source("#BPM 120\n#WAV01 tone\n#00011:01\n#00032:01\n#000D6:1E00\n#000E6:00ZZ\n#000D1:01");
    augmented.mines[0].ordinal = u64::MAX;
    augmented.mines.reverse();
    let before = augmented.clone();
    assert_eq!(augmented.compile().unwrap(), ordinary.compile().unwrap());
    let chart = Arc::new(project(&augmented));
    assert_eq!(augmented, before);
    assert_eq!(chart.lanes, [0x16, 0x11, 0x12, 0x26]);
    assert_eq!(
        chart.mines(),
        [
            PlayerMine {
                ordinal: 2,
                lane_index: 1,
                at: ts(0),
                damage: MineDamage::from_raw(1).unwrap()
            },
            PlayerMine {
                ordinal: u64::MAX,
                lane_index: 0,
                at: ts(0),
                damage: MineDamage::from_raw(50).unwrap()
            },
            PlayerMine {
                ordinal: 1,
                lane_index: 3,
                at: ts(1_000_000_000),
                damage: MineDamage::from_raw(1295).unwrap()
            },
        ]
    );
    assert_eq!(chart.notes.len(), 1);
    assert_eq!(chart.notes[0].object, ObjectId(1));
    assert_eq!(chart.notes[0].lane_index, 1);
    assert!(chart.note_by_object(ObjectId(2)).is_none());
    let progress = NoteProgress::new(Arc::clone(&chart)).unwrap();
    assert_eq!(
        progress.state(0),
        Some(crate::note_progress::NoteState::Pending)
    );
    assert_eq!(progress.state(1), None);
    let mut indices = Vec::new();
    chart
        .visible_mine_indices_checked(ts(0), 0, 0, &mut indices)
        .unwrap();
    assert_eq!(indices, [0, 1]);
    let only = Arc::new(model("#BPM 60\n#001D6:ZZ"));
    assert!(only.notes.is_empty());
    assert_eq!(only.mines()[0].at, ts(4_000_000_000));
    assert_eq!(NoteProgress::new(only).unwrap().state(0), None);
    let plain = project(&ordinary);
    assert!(plain.mines().is_empty());
    let mut unused = ordinary;
    unused.mine_ticks_per_beat = 0;
    assert!(project(&unused).mines().is_empty());
}

#[test]
fn inclusive_wide_windows_match_a_linear_oracle_and_keep_bounded_reusable_scratch() {
    assert_eq!(MAX_VISIBLE_MINES, 2048);
    let chart = model("#BPM 60\n#000D1:01020304\n#000D2:ZZ000000");
    assert_eq!(
        chart
            .mines()
            .iter()
            .map(|mine| (mine.ordinal, mine.at.as_nanos()))
            .collect::<Vec<_>>(),
        [
            (0, 0),
            (4, 0),
            (1, 1_000_000_000),
            (2, 2_000_000_000),
            (3, 3_000_000_000),
        ]
    );
    let mut scratch = vec![usize::MAX];
    for (now, ahead, behind, expected) in [
        (0, 0, 0, vec![0, 1]),
        (
            1_000_000_000,
            1_000_000_000,
            1_000_000_000,
            vec![0, 1, 2, 3],
        ),
        (1_000_000_001, 999_999_998, 0, vec![]),
        (i64::MIN, i64::MAX, i64::MAX, vec![]),
        (i64::MAX, i64::MAX, i64::MAX, vec![0, 1, 2, 3, 4]),
    ] {
        chart
            .visible_mine_indices_checked(ts(now), ahead, behind, &mut scratch)
            .unwrap();
        assert_eq!(scratch, expected);
    }
    // Independent linear membership oracle, not the production binary search.
    let mut seed = 0x6a09_e667_f3bc_c909u64;
    for _ in 0..256 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let now = (seed % 8_000_000_001) as i64 - 2_000_000_000;
        let ahead = ((seed >> 13) % 4_000_000_001) as i64;
        let behind = ((seed >> 29) % 4_000_000_001) as i64;
        let lower = i128::from(now) - i128::from(behind);
        let upper = i128::from(now) + i128::from(ahead);
        let expected: Vec<_> = chart
            .mines()
            .iter()
            .enumerate()
            .filter_map(|(index, mine)| {
                let at = i128::from(mine.at.as_nanos());
                (lower <= at && at <= upper).then_some(index)
            })
            .collect();
        chart
            .visible_mine_indices_checked(ts(now), ahead, behind, &mut scratch)
            .unwrap();
        assert_eq!(scratch, expected);
    }
    for (ahead, behind) in [(-1, 0), (0, -1), (i64::MIN, i64::MIN)] {
        scratch.push(99);
        chart
            .visible_mine_indices_checked(ts(0), ahead, behind, &mut scratch)
            .unwrap();
        assert!(scratch.is_empty());
    }
    let long = model("#BPM 0.001\n#003D1:1E");
    assert_eq!(long.mines()[0].at, ts(720_000_000_000_000));
    long.visible_mine_indices_checked(ts(720_000_000_000_000), 0, 0, &mut scratch)
        .unwrap();
    assert_eq!(scratch, [0]);
    long.visible_mine_indices_checked(ts(720_000_000_000_001), 0, 0, &mut scratch)
        .unwrap();
    assert!(scratch.is_empty());

    let at_limit = model(&format!("#BPM 60\n#000D1:{}", "1E".repeat(2048)));
    at_limit
        .visible_mine_indices_checked(ts(0), i64::MAX, 0, &mut scratch)
        .unwrap();
    assert_eq!(scratch, (0..2048).collect::<Vec<_>>());
    let pointer = scratch.as_ptr();
    let capacity = scratch.capacity();
    let dense = model(&format!("#BPM 60\n#000D1:{}", "1E".repeat(2049)));
    assert!(
        dense
            .visible_mine_indices_checked(ts(0), i64::MAX, 0, &mut scratch)
            .is_err()
    );
    assert!(scratch.is_empty());
    assert_eq!(scratch.as_ptr(), pointer);
    assert_eq!(scratch.capacity(), capacity);
    for now in [0, 1_000_000_000, 4_000_000_000, 0] {
        at_limit
            .visible_mine_indices_checked(ts(now), i64::MAX, 0, &mut scratch)
            .unwrap();
        assert_eq!(scratch.as_ptr(), pointer);
        assert_eq!(scratch.capacity(), capacity);
    }
    let empty = model("#BPM 60");
    empty
        .visible_mine_indices_checked(ts(0), i64::MAX, i64::MAX, &mut scratch)
        .unwrap();
    assert!(scratch.is_empty());
    assert_eq!(scratch.as_ptr(), pointer);
    assert_eq!(scratch.capacity(), capacity);
    let mut fresh = Vec::new();
    for now in [i64::MIN, 0, i64::MAX] {
        empty
            .visible_mine_indices_checked(ts(now), i64::MAX, i64::MAX, &mut fresh)
            .unwrap();
        assert!(fresh.is_empty());
        assert_eq!(fresh.capacity(), 0);
    }
}

#[cfg(feature = "graphics")]
mod rendering {
    use super::*;
    use crate::{
        local_players::PlayerId,
        native_judge::NativeJudgeConfig,
        player::LocalPlayerSnapshot,
        playfield_gpu::{MAX_NOTE_INSTANCES, PlayfieldCache},
        scene::Scene,
        ui::{interaction::Bounds, organisms},
    };
    use beatkernel::{
        input::{
            ButtonEvent, ButtonState, DeviceId, EventMeta, GameControlId, GameInputEvent,
            PhysicalControlId, PhysicalInputEvent,
        },
        time::{ClockDomainId, ClockPoint},
    };

    fn bounds() -> Bounds {
        Bounds {
            x: 80,
            y: 106,
            width: 640,
            height: 528,
        }
    }
    fn appearance(kind: f32, rgb: [u8; 3]) -> [f32; 4] {
        [
            kind,
            f32::from(rgb[0]) / 255.0,
            f32::from(rgb[1]) / 255.0,
            f32::from(rgb[2]) / 255.0,
        ]
    }
    fn judge(source: &BmsChart) -> beatkernel::judge::JudgeEngine {
        NativeJudgeConfig {
            early: 0,
            late: 0,
            offset: 0,
            preroll: 0,
            output: ClockDomainId(7),
            end: None,
        }
        .judge(source, source.compile().unwrap().chart)
        .unwrap()
    }

    #[test]
    fn mixed_cache_keeps_normal_primitives_and_reuses_or_invalidates_exact_retained_packets() {
        let source = mixed_source();
        let chart = Arc::new(project(&source));
        let mut cache = PlayfieldCache::default();
        let mut ordinary_cache = PlayfieldCache::default();
        let baseline = ordinary_cache.frame_indexed_with_progress(
            &chart.notes,
            &[0, 1],
            2,
            bounds(),
            ts(0),
            1_000_000_000,
            None,
        );
        let first = cache.frame_indexed_with_mines_and_progress(
            &chart.notes,
            &[0, 1],
            chart.mines(),
            &[0, 1],
            2,
            bounds(),
            ts(0),
            1_000_000_000,
            None,
        );
        assert_eq!(first.instances.len(), 6);
        for (actual, old) in first.instances[..4].iter().zip(baseline.instances.iter()) {
            assert_eq!(actual.geometry, old.geometry);
            assert_eq!(actual.appearance, old.appearance);
        }
        assert_eq!(first.instances[4].geometry, [83.0, 314.0, 485.0, 485.0]);
        assert_eq!(
            first.instances[4].appearance,
            appearance(2.0, [239, 99, 114])
        );
        assert_eq!(first.instances[5].geometry, [403.0, 314.0, 235.0, 235.0]);
        assert_eq!(
            first.instances[5].appearance,
            appearance(2.0, [216, 107, 255])
        );
        assert_eq!((first.top, first.bottom, first.drift), (110.0, 625.0, 0.0));
        let next = cache.frame_indexed_with_mines_and_progress(
            &chart.notes,
            &[0, 1],
            chart.mines(),
            &[0, 1],
            2,
            bounds(),
            ts(100_000_000),
            1_000_000_000,
            None,
        );
        assert!(Arc::ptr_eq(&first.instances, &next.instances));
        assert_eq!(next.drift, 50.0);
        let mut changed = chart.mines().to_vec();
        changed[0].damage = MineDamage::from_raw(51).unwrap();
        let damage = cache.frame_indexed_with_mines_and_progress(
            &chart.notes,
            &[0, 1],
            &changed,
            &[0, 1],
            2,
            bounds(),
            ts(100_000_000),
            1_000_000_000,
            None,
        );
        assert!(
            !Arc::ptr_eq(&next.instances, &damage.instances),
            "metadata changes invalidate even the same nonfatal color"
        );
        assert_eq!(
            damage.instances[4].appearance,
            first.instances[4].appearance
        );
        changed[0].damage = MineDamage::from_raw(1295).unwrap();
        let fatal = cache.frame_indexed_with_mines_and_progress(
            &chart.notes,
            &[0, 1],
            &changed,
            &[0, 1],
            2,
            bounds(),
            ts(100_000_000),
            1_000_000_000,
            None,
        );
        assert!(!Arc::ptr_eq(&damage.instances, &fatal.instances));
        assert_eq!(
            fatal.instances[4].appearance,
            appearance(2.0, [216, 107, 255])
        );
        let rewind = cache.frame_indexed_with_mines_and_progress(
            &chart.notes,
            &[0, 1],
            &changed,
            &[0, 1],
            2,
            bounds(),
            ts(0),
            1_000_000_000,
            None,
        );
        assert!(!Arc::ptr_eq(&fatal.instances, &rewind.instances));
        let resized = cache.frame_indexed_with_mines_and_progress(
            &chart.notes,
            &[0, 1],
            &changed,
            &[0, 1],
            2,
            Bounds {
                width: 320,
                ..bounds()
            },
            ts(0),
            1_000_000_000,
            None,
        );
        assert!(!Arc::ptr_eq(&rewind.instances, &resized.instances));
        assert_eq!(resized.instances[4].geometry, [83.0, 154.0, 485.0, 485.0]);
        let membership = cache.frame_indexed_with_mines_and_progress(
            &chart.notes,
            &[0, 1],
            &changed,
            &[1],
            2,
            Bounds {
                width: 320,
                ..bounds()
            },
            ts(0),
            1_000_000_000,
            None,
        );
        assert_eq!(membership.instances.len(), 5);
        assert!(!Arc::ptr_eq(&resized.instances, &membership.instances));
        let horizon = cache.frame_indexed_with_mines_and_progress(
            &chart.notes,
            &[0, 1],
            &changed,
            &[1],
            2,
            Bounds {
                width: 320,
                ..bounds()
            },
            ts(0),
            2_000_000_000,
            None,
        );
        assert!(!Arc::ptr_eq(&membership.instances, &horizon.instances));

        let mut progress = NoteProgress::new(Arc::clone(&chart)).unwrap();
        let mut engine = judge(&source);
        let events = engine
            .push_input(
                &GameInputEvent {
                    game_control: GameControlId(0x11),
                    physical: PhysicalInputEvent::Button(ButtonEvent {
                        meta: EventMeta::new(
                            DeviceId(9),
                            ClockPoint {
                                domain: ClockDomainId(7),
                                timestamp: ts(0),
                            },
                            0,
                        ),
                        control: PhysicalControlId::keyboard(30u16),
                        state: ButtonState::Down,
                    }),
                },
                ts(0),
            )
            .unwrap();
        progress.apply(&events);
        assert_eq!(
            progress.state(0),
            Some(crate::note_progress::NoteState::Holding)
        );
        let holding = cache.frame_indexed_with_mines_and_progress(
            &chart.notes,
            &[0, 1],
            chart.mines(),
            &[0, 1],
            2,
            bounds(),
            ts(0),
            1_000_000_000,
            Some(&progress),
        );
        assert_eq!(holding.instances.len(), 5);
        assert_eq!(holding.instances[0].appearance[0], 0.0);
        assert_eq!(holding.instances[1].appearance[0], 1.0);
        assert_eq!(
            holding.instances[3].appearance,
            appearance(2.0, [239, 99, 114])
        );
        assert_eq!(
            holding.instances[4].appearance,
            appearance(2.0, [216, 107, 255])
        );
        let mut distant = chart.mines().to_vec();
        distant[0].at = ts(720_000_250_000_000);
        distant[1].at = ts(720_000_750_000_000);
        let long = cache.frame_indexed_with_mines_and_progress(
            &[],
            &[],
            &distant,
            &[0, 1],
            2,
            bounds(),
            ts(720_000_000_000_000),
            1_000_000_000,
            None,
        );
        assert_eq!(long.instances[0].geometry, first.instances[4].geometry);
        assert_eq!(long.instances[1].geometry, first.instances[5].geometry);
        let later = cache.frame_indexed_with_mines_and_progress(
            &[],
            &[],
            &distant,
            &[0, 1],
            2,
            bounds(),
            ts(720_000_100_000_000),
            1_000_000_000,
            None,
        );
        assert!(Arc::ptr_eq(&long.instances, &later.instances));
        assert_eq!(later.drift, 50.0);
    }

    #[test]
    fn actual_solo_and_all_local_pages_share_mine_packets_without_normal_progress_aliases() {
        let source = mixed_source();
        let chart = Arc::new(project(&source));
        let pending = NoteProgress::new(Arc::clone(&chart)).unwrap();
        let mut completed = pending.clone();
        let mut engine = judge(&source);
        completed.apply(&engine.advance_to(ts(3_000_000_000)).unwrap());
        assert_eq!(
            completed.state(0),
            Some(crate::note_progress::NoteState::Completed)
        );
        assert_eq!(
            completed.state(1),
            Some(crate::note_progress::NoteState::Completed)
        );
        let mut scene = Scene::new(960, 720);
        organisms::playfield_with_progress(
            &mut scene,
            &chart,
            ts(0),
            1_000_000_000,
            &[],
            0,
            Some(&completed),
        )
        .unwrap();
        assert_eq!(scene.playfields().len(), 1);
        assert_eq!(scene.playfields()[0].instances.len(), 2);
        assert_eq!(
            scene.playfields()[0].instances[0].appearance,
            appearance(2.0, [239, 99, 114])
        );
        assert_eq!(
            scene
                .batches()
                .iter()
                .filter(|batch| batch.playfield.is_some())
                .count(),
            1
        );
        for count in [32usize, 64] {
            let players: Vec<_> = (0..count)
                .map(|index| LocalPlayerSnapshot {
                    player: PlayerId(if index + 1 == count {
                        u32::MAX
                    } else {
                        (index as u32 + 1) * 3
                    }),
                    chart: Some(Arc::clone(&chart)),
                    song_time: Some(ts(0)),
                    score: Default::default(),
                    mine_damage: Default::default(),
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
            for page in 0..count / 4 {
                scene.clear();
                organisms::local_players(&mut scene, &players, 1_000_000_000, page).unwrap();
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
                    .map(|frame| Arc::clone(&frame.instances))
                    .collect();
                for (slot, frame) in scene.playfields().iter().enumerate() {
                    assert_eq!(frame.instances.len(), if slot % 2 == 0 { 2 } else { 6 });
                    let end = frame.instances.len();
                    assert_eq!(
                        frame.instances[end - 2].appearance,
                        appearance(2.0, [239, 99, 114])
                    );
                    assert_eq!(
                        frame.instances[end - 1].appearance,
                        appearance(2.0, [216, 107, 255])
                    );
                    let [x, y, width, height] =
                        crate::playfield_layout::local_field_bounds(4, slot).unwrap();
                    assert_eq!(
                        (frame.top, frame.bottom),
                        ((y + 4) as f32, (y + height - 9) as f32)
                    );
                    for chip in &frame.instances[end - 2..] {
                        assert!(chip.geometry[0] >= x as f32);
                        assert!(chip.geometry[0] + chip.geometry[1] <= (x + width) as f32);
                    }
                }
                scene.clear();
                organisms::local_players(&mut scene, &players, 1_000_000_000, page).unwrap();
                for (old, frame) in prior.iter().zip(scene.playfields()) {
                    assert!(Arc::ptr_eq(old, &frame.instances));
                }
            }
            scene.clear();
            assert!(
                organisms::local_players(&mut scene, &players, 1_000_000_000, count / 4).is_err()
            );
            assert!(scene.playfields().is_empty());
        }
    }

    #[test]
    fn scene_rejects_dense_or_invalid_lanes_before_batch_and_cache_changes_and_keeps_legacy_geometry()
     {
        assert_eq!(MAX_NOTE_INSTANCES, 8192);
        let good = model("#BPM 60\n#000D1:1E");
        let mut scene = Scene::new(960, 720);
        scene
            .playfield(&good, ts(0), 1_000_000_000, bounds())
            .unwrap();
        let prior = Arc::clone(&scene.playfields()[0].instances);
        let dense = model(&format!("#BPM 60\n#000D1:{}", "1E".repeat(2049)));
        let mut invalid = good.clone();
        invalid.lanes.clear();
        for rejected in [&dense, &invalid] {
            scene.clear();
            scene.rect(1, 2, 3, 4, 0x010203);
            let batches = scene.batches().len();
            let rectangles = scene.rectangles().len();
            assert!(
                scene
                    .playfield(rejected, ts(0), i64::MAX, bounds())
                    .is_err()
            );
            assert!(scene.playfields().is_empty());
            assert_eq!(scene.batches().len(), batches);
            assert_eq!(scene.rectangles().len(), rectangles);
            scene
                .playfield(&good, ts(0), 1_000_000_000, bounds())
                .unwrap();
            assert!(Arc::ptr_eq(&prior, &scene.playfields()[0].instances));
        }
        let maximum = model(&format!(
            "#BPM 60\n#WAV01 tone\n#LNTYPE 1\n#00051:{}\n#000D1:{}",
            "0101".repeat(2048),
            "1E".repeat(2048)
        ));
        scene.clear();
        scene
            .playfield(&maximum, ts(0), i64::MAX, bounds())
            .unwrap();
        assert_eq!(scene.playfields()[0].instances.len(), 8192);
        assert_eq!(
            scene.playfields()[0].instances[..6144]
                .iter()
                .filter(|n| n.appearance[0] == 0.0)
                .count(),
            2048
        );
        assert!(
            scene.playfields()[0].instances[6144..]
                .iter()
                .all(|n| n.appearance == appearance(2.0, [239, 99, 114]))
        );
        let plain = model("#BPM 120\n#WAV01 tone\n#LNTYPE 1\n#00051:0101");
        scene.clear();
        scene
            .playfield(&plain, ts(0), 1_000_000_000, bounds())
            .unwrap();
        let mut legacy = PlayfieldCache::default();
        let reference = legacy.frame_indexed_with_progress(
            &plain.notes,
            &[0],
            1,
            bounds(),
            ts(0),
            1_000_000_000,
            None,
        );
        for (actual, expected) in scene.playfields()[0]
            .instances
            .iter()
            .zip(reference.instances.iter())
        {
            assert_eq!(actual.geometry, expected.geometry);
            assert_eq!(actual.appearance, expected.appearance);
        }
        assert_eq!(scene.playfields()[0].instances.len(), 3);
        scene.clear();
        scene
            .playfield(&model("#BPM 120"), ts(0), 1_000_000_000, bounds())
            .unwrap();
        assert!(scene.playfields()[0].instances.is_empty());
    }
}
