//! Retained local-epoch note instances. No clocks, devices or gameplay ownership.
use std::sync::Arc;

use beatkernel::time::Timestamp;

use crate::{
    player_chart::{PlayerMine, PlayerNote},
    ui::interaction::Bounds,
};

pub(crate) use crate::presentation_budget::MAX_PLAYFIELDS;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct NoteInstance {
    // x, width, head y at epoch, tail y at epoch
    pub geometry: [f32; 4],
    // primitive kind (body/tail/head), red, green, blue
    pub appearance: [f32; 4],
}

pub(crate) struct PlayfieldFrame {
    pub instances: Arc<[NoteInstance]>,
    pub drift: f32,
    pub top: f32,
    pub bottom: f32,
}

#[derive(Default)]
pub(crate) struct PlayfieldCache {
    state: Option<Cached>,
}

struct Cached {
    notes: Vec<(PlayerNote, bool)>,
    mines: Vec<PlayerMine>,
    lanes: usize,
    bounds: Bounds,
    lookahead: i64,
    epoch: Timestamp,
    previous: Timestamp,
    instances: Arc<[NoteInstance]>,
}

impl PlayfieldCache {
    #[cfg(test)]
    pub fn frame(
        &mut self,
        notes: &[&PlayerNote],
        lanes: usize,
        bounds: Bounds,
        now: Timestamp,
        lookahead: i64,
    ) -> Result<PlayfieldFrame, String> {
        self.frame_inner(
            notes.iter().map(|&note| (note, false)),
            std::iter::empty(),
            lanes,
            bounds,
            now,
            lookahead,
        )
    }

    #[cfg(test)]
    pub fn frame_indexed(
        &mut self,
        notes: &[PlayerNote],
        indices: &[usize],
        lanes: usize,
        bounds: Bounds,
        now: Timestamp,
        lookahead: i64,
    ) -> Result<PlayfieldFrame, String> {
        self.frame_indexed_with_progress(notes, indices, lanes, bounds, now, lookahead, None)
    }

    pub fn frame_indexed_with_progress(
        &mut self,
        notes: &[PlayerNote],
        indices: &[usize],
        lanes: usize,
        bounds: Bounds,
        now: Timestamp,
        lookahead: i64,
        progress: Option<&crate::note_progress::NoteProgress>,
    ) -> Result<PlayfieldFrame, String> {
        self.frame_indexed_with_mines_and_progress(
            notes,
            indices,
            &[],
            &[],
            lanes,
            bounds,
            now,
            lookahead,
            progress,
        )
    }

    pub(crate) fn frame_indexed_with_mines_and_progress(
        &mut self,
        notes: &[PlayerNote],
        indices: &[usize],
        mines: &[PlayerMine],
        mine_indices: &[usize],
        lanes: usize,
        bounds: Bounds,
        now: Timestamp,
        lookahead: i64,
        progress: Option<&crate::note_progress::NoteProgress>,
    ) -> Result<PlayfieldFrame, String> {
        crate::presentation_budget::source_extent(notes.len(), mines.len(), 0, 0)
            .map_err(str::to_string)?;
        if indices.iter().any(|&index| index >= notes.len()) {
            return Err("playfield note index is unavailable".into());
        }
        if mine_indices.iter().any(|&index| index >= mines.len()) {
            return Err("playfield mine index is unavailable".into());
        }
        if progress.is_some_and(|state| indices.iter().any(|&index| state.state(index).is_none())) {
            return Err("playfield progress index is unavailable".into());
        }
        if indices
            .iter()
            .any(|&index| notes[index].lane_index >= lanes)
            || mine_indices
                .iter()
                .any(|&index| mines[index].lane_index >= lanes)
        {
            return Err("playfield object references an unavailable lane".into());
        }
        self.frame_inner(
            indices
                .iter()
                .filter(|&&index| {
                    !progress.is_some_and(|state| {
                        state.state(index) == Some(crate::note_progress::NoteState::Completed)
                    })
                })
                .map(|&index| {
                    (
                        &notes[index],
                        progress.is_some_and(|state| {
                            state.state(index) == Some(crate::note_progress::NoteState::Holding)
                        }),
                    )
                }),
            mine_indices.iter().map(|&index| &mines[index]),
            lanes,
            bounds,
            now,
            lookahead,
        )
    }

    fn frame_inner<'a>(
        &mut self,
        notes: impl Iterator<Item = (&'a PlayerNote, bool)> + Clone,
        mines: impl Iterator<Item = &'a PlayerMine> + Clone,
        lanes: usize,
        bounds: Bounds,
        now: Timestamp,
        lookahead: i64,
    ) -> Result<PlayfieldFrame, String> {
        if lookahead <= 0 {
            return Err("playfield lookahead must be positive".into());
        }
        if notes.clone().any(|(note, _)| note.lane_index >= lanes)
            || mines.clone().any(|mine| mine.lane_index >= lanes)
        {
            return Err("playfield object references an unavailable lane".into());
        }
        if bounds.width <= 0
            || bounds.height <= 28
            || bounds
                .x
                .checked_add(bounds.width)
                .and_then(|right| right.checked_add(6))
                .is_none()
        {
            return Err("playfield bounds are invalid".into());
        }
        let top = bounds.y.checked_add(4).ok_or("playfield top overflow")?;
        let bottom = bounds
            .y
            .checked_add(bounds.height)
            .ok_or("playfield bottom overflow")?;
        let line = bottom.checked_sub(24).ok_or("playfield line overflow")?;
        let note_count = notes.clone().count();
        let mine_count = mines.clone().count();
        crate::presentation_budget::source_extent(note_count, mine_count, 0, 0)
            .map_err(str::to_string)?;
        let reuse = self.state.as_ref().is_some_and(|cached| {
            cached.lanes == lanes
                && cached.bounds.x == bounds.x
                && cached.bounds.y == bounds.y
                && cached.bounds.width == bounds.width
                && cached.bounds.height == bounds.height
                && cached.lookahead == lookahead
                && now >= cached.previous
                && i128::from(now.as_nanos()) - i128::from(cached.epoch.as_nanos())
                    <= i128::from(lookahead) / 4
                && cached.notes.len() == note_count
                && cached
                    .notes
                    .iter()
                    .zip(notes.clone())
                    .all(|(old, (new, consumed))| old.0 == *new && old.1 == consumed)
                && cached.mines.len() == mine_count
                && cached
                    .mines
                    .iter()
                    .zip(mines.clone())
                    .all(|(old, new)| old == new)
        });
        if !reuse {
            let instance_count = notes
                .clone()
                .try_fold(mine_count, |count, (note, consumed)| {
                    count.checked_add(usize::from(note.end.is_some()) * 2 + usize::from(!consumed))
                })
                .ok_or("presentation instance count overflow")?;
            crate::presentation_budget::instance_extent(
                instance_count,
                std::mem::size_of::<NoteInstance>(),
            )
            .map_err(str::to_string)?;
            // Stage every fallible reservation before publishing the replacement.
            let mut instances = Vec::new();
            let mut retained_notes = Vec::new();
            let mut retained_mines = Vec::new();
            instances
                .try_reserve_exact(instance_count)
                .map_err(|_| "playfield instance allocation failed")?;
            retained_notes
                .try_reserve_exact(note_count)
                .map_err(|_| "playfield note cache allocation failed")?;
            retained_mines
                .try_reserve_exact(mine_count)
                .map_err(|_| "playfield mine cache allocation failed")?;
            // Outside this margin no endpoint can enter the clip region before
            // rebasing. Saturating a week-long hold thus avoids huge GPU floats
            // while preserving its visible body and eventual endpoint.
            let margin = (line - top) as f64 + 32.0;
            let position = |time: Timestamp| {
                let delta = i128::from(time.as_nanos()) - i128::from(now.as_nanos());
                (line as f64 - delta as f64 * (line - top) as f64 / lookahead as f64)
                    .clamp(top as f64 - margin, line as f64 + 15.0 + margin) as f32
            };
            for (note, head_consumed) in notes.clone() {
                let left = bounds.x
                    + (note.lane_index as i128 * i128::from(bounds.width) / lanes as i128) as i64;
                let right = bounds.x
                    + ((note.lane_index + 1) as i128 * i128::from(bounds.width) / lanes as i128)
                        as i64;
                let head = position(note.start);
                let tail = note.end.map_or(head, position);
                let mut push = |x: i64, width: i64, kind: f32, color: u32| {
                    instances.push(NoteInstance {
                        geometry: [x as f32, width.max(1) as f32, head, tail],
                        appearance: [
                            kind,
                            ((color >> 16) & 255) as f32 / 255.0,
                            ((color >> 8) & 255) as f32 / 255.0,
                            (color & 255) as f32 / 255.0,
                        ],
                    });
                };
                if note.end.is_some() {
                    push(left + 6, right - left - 12, 0.0, 0x357e98);
                    push(left + 3, right - left - 6, 1.0, 0x87e7ff);
                }
                if !head_consumed {
                    push(left + 3, right - left - 6, 2.0, 0x74e5c5);
                }
            }
            for mine in mines.clone() {
                let left = bounds.x
                    + (mine.lane_index as i128 * i128::from(bounds.width) / lanes as i128) as i64;
                let right = bounds.x
                    + ((mine.lane_index + 1) as i128 * i128::from(bounds.width) / lanes as i128)
                        as i64;
                let head = position(mine.at);
                let color: u32 = if mine.damage.is_fatal() {
                    0xd86bff
                } else {
                    0xef6372
                };
                instances.push(NoteInstance {
                    geometry: [
                        (left + 3) as f32,
                        (right - left - 6).max(1) as f32,
                        head,
                        head,
                    ],
                    appearance: [
                        2.0,
                        ((color >> 16) & 255) as f32 / 255.0,
                        ((color >> 8) & 255) as f32 / 255.0,
                        (color & 255) as f32 / 255.0,
                    ],
                });
            }
            retained_notes.extend(notes.map(|(note, consumed)| (note.clone(), consumed)));
            retained_mines.extend(mines.cloned());
            let instances = instances.into();
            self.state = Some(Cached {
                notes: retained_notes,
                mines: retained_mines,
                lanes,
                bounds,
                lookahead,
                epoch: now,
                previous: now,
                instances,
            });
        }
        let cached = self.state.as_mut().expect("cache installed above");
        cached.previous = now;
        let delta = i128::from(now.as_nanos()) - i128::from(cached.epoch.as_nanos());
        Ok(PlayfieldFrame {
            instances: Arc::clone(&cached.instances),
            drift: (delta as f64 * (line - top) as f64 / lookahead as f64) as f32,
            top: top as f32,
            bottom: (line + 15) as f32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatkernel::chart::ObjectId;

    fn bounds() -> Bounds {
        Bounds {
            x: 80,
            y: 106,
            width: 640,
            height: 528,
        }
    }
    fn note(start: i64, end: Option<i64>) -> PlayerNote {
        PlayerNote {
            object: ObjectId(1),
            lane_index: 0,
            start: Timestamp::from_nanos(start),
            end: end.map(Timestamp::from_nanos),
        }
    }

    #[test]
    fn indexed_and_reference_frames_share_geometry_and_keep_cache_semantics() {
        let notes = vec![
            note(0, Some(2_000_000_000)),
            PlayerNote {
                object: ObjectId(2),
                ..note(500_000_000, None)
            },
        ];
        let indices = [0, 1];
        let mut indexed = PlayfieldCache::default();
        let mut legacy = PlayfieldCache::default();
        let first = indexed
            .frame_indexed(
                &notes,
                &indices,
                1,
                bounds(),
                Timestamp::ZERO,
                1_000_000_000,
            )
            .unwrap();
        let reference = legacy
            .frame(
                &[&notes[0], &notes[1]],
                1,
                bounds(),
                Timestamp::ZERO,
                1_000_000_000,
            )
            .unwrap();
        assert_eq!(first.instances.len(), 4);
        for (a, b) in first.instances.iter().zip(reference.instances.iter()) {
            assert_eq!(a.geometry, b.geometry);
            assert_eq!(a.appearance, b.appearance);
        }
        let next = indexed
            .frame_indexed(
                &notes,
                &indices,
                1,
                bounds(),
                Timestamp::from_nanos(100_000_000),
                1_000_000_000,
            )
            .unwrap();
        assert!(Arc::ptr_eq(&first.instances, &next.instances));
        let replacement = vec![
            PlayerNote {
                object: ObjectId(3),
                ..notes[0].clone()
            },
            notes[1].clone(),
        ];
        let changed = indexed
            .frame_indexed(
                &replacement,
                &indices,
                1,
                bounds(),
                Timestamp::from_nanos(100_000_000),
                1_000_000_000,
            )
            .unwrap();
        assert!(!Arc::ptr_eq(&next.instances, &changed.instances));
        let seek = indexed
            .frame_indexed(
                &replacement,
                &indices,
                1,
                bounds(),
                Timestamp::ZERO,
                1_000_000_000,
            )
            .unwrap();
        assert!(!Arc::ptr_eq(&changed.instances, &seek.instances));
        let empty = indexed
            .frame_indexed(
                &replacement,
                &[],
                1,
                bounds(),
                Timestamp::ZERO,
                1_000_000_000,
            )
            .unwrap();
        assert!(empty.instances.is_empty());
    }

    #[test]
    fn empty_zero_lane_field_succeeds_and_nonempty_rejection_preserves_cache() {
        let mut cache = PlayfieldCache::default();
        let empty = cache
            .frame_indexed(&[], &[], 0, bounds(), Timestamp::ZERO, 1_000_000_000)
            .unwrap();
        assert!(empty.instances.is_empty());
        let notes = [note(0, None)];
        assert!(cache
            .frame_indexed(&notes, &[0], 0, bounds(), Timestamp::ZERO, 1_000_000_000)
            .is_err());
        assert!(Arc::ptr_eq(
            &empty.instances,
            &cache.state.as_ref().unwrap().instances
        ));
        assert!(cache
            .frame(&[], 0, bounds(), Timestamp::ZERO, 1_000_000_000)
            .unwrap()
            .instances
            .is_empty());
        assert!(cache
            .frame(&[&notes[0]], 0, bounds(), Timestamp::ZERO, 1_000_000_000)
            .is_err());
        assert!(Arc::ptr_eq(
            &empty.instances,
            &cache.state.as_ref().unwrap().instances
        ));
    }

    #[test]
    fn invalid_candidate_keeps_previous_cache_and_epoch() {
        let notes = vec![note(0, Some(2_000_000_000))];
        let mut cache = PlayfieldCache::default();
        let first = cache
            .frame_indexed(&notes, &[0], 1, bounds(), Timestamp::ZERO, 1_000_000_000)
            .unwrap();
        for indices in [&[usize::MAX][..], &[1][..]] {
            assert!(cache
                .frame_indexed(
                    &notes,
                    indices,
                    1,
                    bounds(),
                    Timestamp::from_nanos(100_000_000),
                    1_000_000_000
                )
                .is_err());
            assert!(Arc::ptr_eq(
                &first.instances,
                &cache.state.as_ref().unwrap().instances
            ));
            assert_eq!(cache.state.as_ref().unwrap().previous, Timestamp::ZERO);
        }
        assert!(cache
            .frame_indexed(&notes, &[0], 0, bounds(), Timestamp::ZERO, 1_000_000_000)
            .is_err());
        assert!(cache
            .frame_indexed(
                &notes,
                &[0],
                1,
                Bounds {
                    x: i64::MAX,
                    ..bounds()
                },
                Timestamp::ZERO,
                1_000_000_000
            )
            .is_err());
        assert!(cache
            .frame_indexed(&notes, &[0], 1, bounds(), Timestamp::ZERO, 0)
            .is_err());
        let recovered = cache
            .frame_indexed(
                &notes,
                &[0],
                1,
                bounds(),
                Timestamp::from_nanos(100_000_000),
                1_000_000_000,
            )
            .unwrap();
        assert!(Arc::ptr_eq(&first.instances, &recovered.instances));
        assert_eq!(recovered.instances.len(), 3);
    }

    #[test]
    fn steady_frames_reuse_instances_but_seek_geometry_and_epoch_invalidate() {
        let mut cache = PlayfieldCache::default();
        let note = note(1_000_000_000, None);
        let first = cache
            .frame(&[&note], 1, bounds(), Timestamp::ZERO, 1_000_000_000)
            .unwrap();
        let next = cache
            .frame(
                &[&note],
                1,
                bounds(),
                Timestamp::from_nanos(100_000_000),
                1_000_000_000,
            )
            .unwrap();
        assert!(Arc::ptr_eq(&first.instances, &next.instances));
        assert_eq!(next.drift, 50.0);
        let seek = cache
            .frame(&[&note], 1, bounds(), Timestamp::ZERO, 1_000_000_000)
            .unwrap();
        assert!(!Arc::ptr_eq(&next.instances, &seek.instances));
        let epoch = cache
            .frame(
                &[&note],
                1,
                bounds(),
                Timestamp::from_nanos(250_000_001),
                1_000_000_000,
            )
            .unwrap();
        assert!(!Arc::ptr_eq(&seek.instances, &epoch.instances));
        let resize = cache
            .frame(
                &[&note],
                1,
                Bounds {
                    width: 320,
                    ..bounds()
                },
                Timestamp::from_nanos(250_000_001),
                1_000_000_000,
            )
            .unwrap();
        assert!(!Arc::ptr_eq(&epoch.instances, &resize.instances));
    }

    #[test]
    fn long_absolute_times_keep_local_precision_and_spanning_hold_geometry() {
        for origin in [
            72_000_000_000_000,
            604_800_000_000_000,
            i64::MAX - 2_000_000_000,
        ] {
            let note = note(origin + 1_000_000, None);
            let mut cache = PlayfieldCache::default();
            let frame = cache
                .frame(
                    &[&note],
                    1,
                    bounds(),
                    Timestamp::from_nanos(origin),
                    1_000_000_000,
                )
                .unwrap();
            assert_eq!(frame.instances[0].geometry[2], 609.5);
        }
        let hold = note(i64::MIN, Some(i64::MAX));
        let mut cache = PlayfieldCache::default();
        let frame = cache
            .frame(&[&hold], 1, bounds(), Timestamp::ZERO, 1_000_000_000)
            .unwrap();
        assert_eq!(frame.instances.len(), 3);
        assert!(frame.instances[0].geometry[2] > frame.bottom);
        assert!(frame.instances[0].geometry[3] < frame.top);
        assert!(frame
            .instances
            .iter()
            .flat_map(|note| note.geometry)
            .all(f32::is_finite));
    }
}
