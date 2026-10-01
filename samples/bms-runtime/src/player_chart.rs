//! Bounded chart catalog and exact-time data for desktop presentation.
use beatkernel::{
    chart::{CompiledChart, ObjectId},
    time::Timestamp,
};
use beatkernel_bms::BmsChart;
use std::{
    collections::BTreeMap,
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

/// Maximum number of notes returned for one desktop frame.
pub const MAX_VISIBLE_NOTES: usize = 2048;

/// A note from the actual compiled gameplay chart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerNote {
    /// Chart-local gameplay identity.
    pub object: ObjectId,
    /// Index into the presentation's ordered lane list.
    pub lane_index: usize,
    /// Exact compiled head time.
    pub start: Timestamp,
    /// Exact compiled hold endpoint, if present.
    pub end: Option<Timestamp>,
}

/// Presentation data prepared once, outside gameplay/audio callbacks.
#[derive(Clone, Debug)]
pub struct PlayerChart {
    /// Original Unicode title.
    pub title: String,
    /// Original Unicode artist.
    pub artist: String,
    /// Original BMS channels in left-to-right scratch/key order.
    pub lanes: Vec<u8>,
    /// Notes ordered by compiled head timestamp, then identity.
    pub notes: Vec<PlayerNote>,
    /// Latest gameplay endpoint, in song nanoseconds.
    pub duration_ns: i64,
    // A range-maximum tree prunes ended holds without scanning the old prefix.
    endpoint_tree: Vec<i64>,
    tree_leaves: usize,
    object_index: Vec<(ObjectId, usize)>,
    bga: crate::bga::BgaTimeline,
}

/// A chart/catalog preparation failure with a user-visible explanation.
#[derive(Debug)]
pub struct PlayerChartError(pub String);
impl fmt::Display for PlayerChartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl Error for PlayerChartError {}

fn lane_order(channel: u8) -> (u8, u8) {
    let side = channel >> 4;
    let column = channel & 15;
    // Scratch belongs at the outside edge on each side of a double-play chart.
    let rank = if column == 6 {
        if side == 1 { 0 } else { 10 }
    } else {
        column
    };
    (side, rank)
}

impl PlayerChart {
    /// Projects actual compiled IDs and times through adapter-owned lane data.
    pub fn from_compiled(
        source: &BmsChart,
        chart: &CompiledChart,
    ) -> Result<Self, PlayerChartError> {
        let mut by_id = BTreeMap::new();
        for note in &source.notes {
            if by_id.insert(note.object, note.lane.channel()).is_some() {
                return Err(PlayerChartError(format!(
                    "duplicate BMS object {}",
                    note.object.0
                )));
            }
        }
        if by_id.len() != chart.objects().len() {
            return Err(PlayerChartError(
                "BMS lane mapping and compiled chart differ".into(),
            ));
        }
        let mut lanes: Vec<u8> = by_id.values().copied().collect();
        lanes.sort_by_key(|channel| lane_order(*channel));
        lanes.dedup();
        let lane_indices: BTreeMap<_, _> = lanes
            .iter()
            .enumerate()
            .map(|(index, lane)| (*lane, index))
            .collect();
        let mut notes = Vec::with_capacity(chart.objects().len());
        for object in chart.objects() {
            let lane = by_id.remove(&object.id).ok_or_else(|| {
                PlayerChartError(format!("missing BMS lane for object {}", object.id.0))
            })?;
            notes.push(PlayerNote {
                object: object.id,
                lane_index: lane_indices[&lane],
                start: object.time.start,
                end: object.time.end,
            });
        }
        notes.sort_by_key(|note| (note.start, note.object));
        let mut object_index: Vec<_> = notes
            .iter()
            .enumerate()
            .map(|(index, note)| (note.object, index))
            .collect();
        object_index.sort_unstable_by_key(|entry| entry.0);
        let tree_leaves = notes.len().max(1).next_power_of_two();
        let mut endpoint_tree = vec![i64::MIN; tree_leaves * 2];
        let mut duration_ns = 0;
        for (index, note) in notes.iter().enumerate() {
            let end = note.end.unwrap_or(note.start).as_nanos();
            endpoint_tree[tree_leaves + index] = end;
            duration_ns = duration_ns.max(end);
        }
        for index in (1..tree_leaves).rev() {
            endpoint_tree[index] = endpoint_tree[index * 2].max(endpoint_tree[index * 2 + 1]);
        }
        Ok(Self {
            title: source.metadata.get("TITLE").cloned().unwrap_or_default(),
            artist: source.metadata.get("ARTIST").cloned().unwrap_or_default(),
            lanes,
            notes,
            duration_ns,
            endpoint_tree,
            tree_leaves,
            object_index,
            bga: crate::bga::BgaTimeline::from_chart(source).map_err(PlayerChartError)?,
        })
    }

    /// Image selections at the original song time, shared by every session mode.
    pub fn bga_state(&self, now: Timestamp) -> crate::bga::BgaState {
        self.bga.state_at(now)
    }

    /// Looks up a prepared object identity without scanning notes. A changed
    /// public note vector cannot turn a stale prepared index into a panic or
    /// an unrelated object's mapping.
    pub fn note_by_object(&self, object: ObjectId) -> Option<&PlayerNote> {
        self.notes.get(self.note_index_by_object(object)?)
    }

    /// Looks up a prepared note index with the same stale-index identity guard.
    pub fn note_index_by_object(&self, object: ObjectId) -> Option<usize> {
        let entry = self
            .object_index
            .binary_search_by_key(&object, |entry| entry.0)
            .ok()?;
        let index = self.object_index[entry].1;
        (self.notes.get(index)?.object == object).then_some(index)
    }

    /// Returns bounded head/body overlaps in the inclusive requested time window.
    /// Negative window extents return no notes. Wide arithmetic preserves windows
    /// that extend beyond the representable timestamp range.
    pub fn visible_notes(
        &self,
        now: Timestamp,
        lookahead_ns: i64,
        behind_ns: i64,
        max: usize,
    ) -> Vec<&PlayerNote> {
        self.visible_notes_inner(now, lookahead_ns, behind_ns, max.min(MAX_VISIBLE_NOTES))
    }

    /// Presentation admission checks one extra overlap instead of silently
    /// truncating dense charts. Gameplay judging is independent of this budget.
    pub fn visible_notes_checked(
        &self,
        now: Timestamp,
        lookahead_ns: i64,
        behind_ns: i64,
    ) -> Result<Vec<&PlayerNote>, String> {
        let notes = self.visible_notes_inner(now, lookahead_ns, behind_ns, MAX_VISIBLE_NOTES + 1);
        if notes.len() > MAX_VISIBLE_NOTES {
            Err(format!(
                "playfield exceeds {MAX_VISIBLE_NOTES} visible notes"
            ))
        } else {
            Ok(notes)
        }
    }

    /// Writes ordered chart-local indices into reusable bounded scratch storage.
    /// Clears output on every call, including negative windows and rejection.
    /// Successful warmed queries reuse capacity; this is not a whole-frame
    /// allocation guarantee for instance rebuilds or error strings.
    pub fn visible_note_indices_checked(
        &self,
        now: Timestamp,
        lookahead_ns: i64,
        behind_ns: i64,
        output: &mut Vec<usize>,
    ) -> Result<(), String> {
        self.visible_note_indices_with_progress_checked(now, lookahead_ns, behind_ns, None, output)
    }

    /// Filters authoritative completed objects before the visible-note budget.
    /// Foreign progress is rejected; output is cleared on every rejection.
    pub fn visible_note_indices_with_progress_checked(
        &self,
        now: Timestamp,
        lookahead_ns: i64,
        behind_ns: i64,
        progress: Option<&crate::note_progress::NoteProgress>,
        output: &mut Vec<usize>,
    ) -> Result<(), String> {
        output.clear();
        if progress.is_some_and(|state| !state.matches_chart(self)) {
            return Err("note progress belongs to another prepared chart".into());
        }
        let Some((lower, end)) = self.visible_bounds(now, lookahead_ns, behind_ns) else {
            return Ok(());
        };
        let capacity = if end == 0 { 0 } else { MAX_VISIBLE_NOTES + 1 };
        if output.capacity() < capacity {
            output
                .try_reserve_exact(capacity)
                .map_err(|_| "playfield visibility allocation failed".to_string())?;
        }
        self.visit_visible(lower, end, MAX_VISIBLE_NOTES + 1, progress, |index| {
            if progress.is_some_and(|state| {
                state.state(index) == Some(crate::note_progress::NoteState::Completed)
            }) {
                return false;
            }
            output.push(index);
            true
        });
        if output.len() > MAX_VISIBLE_NOTES {
            output.clear();
            return Err(format!(
                "playfield exceeds {MAX_VISIBLE_NOTES} visible notes"
            ));
        }
        Ok(())
    }

    fn visible_bounds(
        &self,
        now: Timestamp,
        lookahead_ns: i64,
        behind_ns: i64,
    ) -> Option<(i128, usize)> {
        if lookahead_ns < 0 || behind_ns < 0 {
            return None;
        }
        let lower = i128::from(now.as_nanos()) - i128::from(behind_ns);
        let upper = i128::from(now.as_nanos()) + i128::from(lookahead_ns);
        let end = self
            .notes
            .partition_point(|note| i128::from(note.start.as_nanos()) <= upper);
        Some((lower, end))
    }

    fn visible_notes_inner(
        &self,
        now: Timestamp,
        lookahead_ns: i64,
        behind_ns: i64,
        max: usize,
    ) -> Vec<&PlayerNote> {
        if max == 0 {
            return Vec::new();
        }
        let Some((lower, end)) = self.visible_bounds(now, lookahead_ns, behind_ns) else {
            return Vec::new();
        };
        let mut visible = Vec::with_capacity(max.min(end));
        self.visit_visible(lower, end, max, None, |index| {
            visible.push(&self.notes[index]);
            true
        });
        visible
    }

    fn visit_visible(
        &self,
        lower: i128,
        end: usize,
        max: usize,
        progress: Option<&crate::note_progress::NoteProgress>,
        mut emit: impl FnMut(usize) -> bool,
    ) {
        let mut emitted = 0;
        self.collect_visible(
            1,
            0,
            self.tree_leaves,
            end,
            lower,
            max,
            progress,
            &mut emitted,
            &mut emit,
        );
    }

    fn collect_visible(
        &self,
        node: usize,
        first: usize,
        last: usize,
        end: usize,
        lower: i128,
        max: usize,
        progress: Option<&crate::note_progress::NoteProgress>,
        emitted: &mut usize,
        emit: &mut impl FnMut(usize) -> bool,
    ) {
        if first >= end || *emitted >= max || i128::from(self.endpoint_tree[node]) < lower {
            return;
        }
        if progress
            .is_some_and(|state| state.all_completed(first, last.min(end).min(self.notes.len())))
        {
            return;
        }
        if last - first == 1 {
            if first < self.notes.len() {
                if emit(first) {
                    *emitted += 1;
                }
            }
            return;
        }
        let middle = first + (last - first) / 2;
        self.collect_visible(
            node * 2,
            first,
            middle,
            end,
            lower,
            max,
            progress,
            emitted,
            emit,
        );
        self.collect_visible(
            node * 2 + 1,
            middle,
            last,
            end,
            lower,
            max,
            progress,
            emitted,
            emit,
        );
    }
}

/// A selectable chart without loaded audio assets.
#[derive(Clone, Debug)]
pub struct LibraryEntry {
    /// Actual chart path.
    pub path: PathBuf,
    /// Original title, or file name if absent.
    pub title: String,
    /// Original artist.
    pub artist: String,
}

/// Bounded library contents and visible parse/traversal diagnostics.
#[derive(Clone, Debug)]
pub struct ChartLibrary {
    /// Deterministically sorted supported charts.
    pub entries: Vec<LibraryEntry>,
    /// Errors and reached limits; sound assets are never opened.
    pub diagnostics: Vec<String>,
}

/// Scans an explicit directory without following symlinks or loading WAV files.
/// Limits: 128 directories, depth eight, 1024 chart files, 8192 directory entries
/// and 64 MiB aggregate advertised chart bytes (individual reader cap: 8 MiB).
pub fn scan_library(root: &Path) -> Result<ChartLibrary, PlayerChartError> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| PlayerChartError(format!("{}: {error}", root.display())))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PlayerChartError(
            "library root must be a non-symlink directory".into(),
        ));
    }
    let mut library = ChartLibrary {
        entries: Vec::new(),
        diagnostics: Vec::new(),
    };
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut directories = 1usize;
    let mut visited = 0usize;
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut entry_limit = false;
    'scan: while let Some((directory, depth)) = pending.pop() {
        let reader = match fs::read_dir(&directory) {
            Ok(reader) => reader,
            Err(error) => {
                library
                    .diagnostics
                    .push(format!("{}: {error}", directory.display()));
                continue;
            }
        };
        let mut paths = Vec::new();
        for entry in reader {
            if visited == 8192 {
                library
                    .diagnostics
                    .push("library directory-entry limit reached (8192)".into());
                entry_limit = true;
                break;
            }
            visited += 1;
            match entry {
                Ok(entry) => paths.push(entry.path()),
                Err(error) => library
                    .diagnostics
                    .push(format!("{}: {error}", directory.display())),
            }
        }
        paths.sort();
        let mut children = Vec::new();
        for path in paths {
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    library
                        .diagnostics
                        .push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                if depth == 8 || directories == 128 {
                    library.diagnostics.push(format!(
                        "{}: library directory/depth limit reached",
                        path.display()
                    ));
                } else {
                    directories += 1;
                    children.push((path, depth + 1));
                }
                continue;
            }
            if !metadata.is_file()
                || !path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        matches!(
                            extension.to_ascii_lowercase().as_str(),
                            "bms" | "bme" | "bml"
                        )
                    })
            {
                continue;
            }
            if files == 1024 {
                library
                    .diagnostics
                    .push("library chart-file limit reached (1024)".into());
                break 'scan;
            }
            files += 1;
            if metadata.len() > 8 * 1024 * 1024 {
                library.diagnostics.push(format!(
                    "{}: BMS text exceeds parser byte cap",
                    path.display()
                ));
                continue;
            }
            if bytes + metadata.len() > 64 * 1024 * 1024 {
                library
                    .diagnostics
                    .push("library aggregate chart-byte limit reached (64 MiB)".into());
                break 'scan;
            }
            bytes += metadata.len();
            match crate::competition_live::load_chart(&path) {
                Ok(chart) => library.entries.push(LibraryEntry {
                    title: chart
                        .metadata
                        .get("TITLE")
                        .filter(|title| !title.is_empty())
                        .cloned()
                        .unwrap_or_else(|| {
                            path.file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned()
                        }),
                    artist: chart.metadata.get("ARTIST").cloned().unwrap_or_default(),
                    path,
                }),
                Err(error) => library
                    .diagnostics
                    .push(format!("{}: {error}", path.display())),
            }
        }
        if entry_limit {
            break;
        }
        pending.extend(children.into_iter().rev());
    }
    library.entries.sort_by(|left, right| {
        (&left.title, &left.artist, &left.path).cmp(&(&right.title, &right.artist, &right.path))
    });
    Ok(library)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn indexed_model(mut notes: Vec<PlayerNote>) -> PlayerChart {
        notes.sort_by_key(|note| (note.start, note.object));
        let mut object_index: Vec<_> = notes
            .iter()
            .enumerate()
            .map(|(index, note)| (note.object, index))
            .collect();
        object_index.sort_unstable_by_key(|entry| entry.0);
        let tree_leaves = notes.len().max(1).next_power_of_two();
        let mut endpoint_tree = vec![i64::MIN; tree_leaves * 2];
        for (index, note) in notes.iter().enumerate() {
            endpoint_tree[tree_leaves + index] = note.end.unwrap_or(note.start).as_nanos();
        }
        for index in (1..tree_leaves).rev() {
            endpoint_tree[index] = endpoint_tree[index * 2].max(endpoint_tree[index * 2 + 1]);
        }
        PlayerChart {
            title: String::new(),
            artist: String::new(),
            lanes: vec![0x11],
            duration_ns: 0,
            notes,
            endpoint_tree,
            tree_leaves,
            object_index,
            bga: crate::bga::BgaTimeline::default(),
        }
    }

    #[test]
    fn completed_long_hold_subtrees_skip_leaf_callbacks_and_preserve_unfinished_neighbors() {
        use crate::note_progress::NoteProgress;
        use beatkernel::judge::{JudgeEvent, JudgeGrade, JudgeOutcome, JudgeStage};
        use beatkernel::time::Duration;
        use std::sync::Arc;
        let count = 4096 * 2 + 17;
        let chart = Arc::new(indexed_model(
            (0..count)
                .map(|index| PlayerNote {
                    object: ObjectId(index as u64 + 1),
                    lane_index: 0,
                    start: Timestamp::ZERO,
                    end: Some(Timestamp::from_nanos(604_800_000_000_000)),
                })
                .collect(),
        ));
        let mut progress = NoteProgress::new(chart.clone()).unwrap();
        let event = |index: usize| JudgeEvent {
            object: chart.notes[index].object,
            stage: JudgeStage::HoldTail,
            outcome: JudgeOutcome::Hit {
                grade: JudgeGrade(1),
                delta: Duration::ZERO,
            },
            at: Timestamp::ZERO,
            input: None,
        };
        progress.apply(&(0..4096).map(event).collect::<Vec<_>>());
        let retained = progress.clone();
        let mut callbacks = Vec::new();
        chart.visit_visible(0, 4100, usize::MAX, Some(&progress), |index| {
            callbacks.push(index);
            true
        });
        assert_eq!(callbacks, vec![4096, 4097, 4098, 4099]);
        progress.apply(&(4096..count).map(event).collect::<Vec<_>>());
        let mut callbacks = 0;
        chart.visit_visible(0, count, usize::MAX, Some(&progress), |_| {
            callbacks += 1;
            true
        });
        assert_eq!(callbacks, 0);
        assert!(progress.all_completed(0, count));
        assert!(!retained.all_completed(0, count));
        let mut visible = Vec::new();
        chart
            .visible_note_indices_with_progress_checked(
                Timestamp::ZERO,
                i64::MAX,
                0,
                Some(&progress),
                &mut visible,
            )
            .unwrap();
        assert!(visible.is_empty());
        let mut legacy = 0;
        chart.visit_visible(0, count, 3, None, |_| {
            legacy += 1;
            true
        });
        assert_eq!(legacy, 3);
    }

    #[test]
    fn completed_filter_precedes_budget_and_foreign_progress_clears_output() {
        use crate::note_progress::NoteProgress;
        use beatkernel::judge::{JudgeEvent, JudgeGrade, JudgeOutcome, JudgeStage};
        use beatkernel::time::Duration;
        use std::sync::Arc;
        let chart = Arc::new(indexed_model(
            (0..MAX_VISIBLE_NOTES + 2)
                .map(|index| PlayerNote {
                    object: ObjectId(index as u64 + 1),
                    lane_index: 0,
                    start: Timestamp::ZERO,
                    end: None,
                })
                .collect(),
        ));
        let mut progress = NoteProgress::new(chart.clone()).unwrap();
        let mut output = vec![99];
        assert!(
            chart
                .visible_note_indices_with_progress_checked(
                    Timestamp::ZERO,
                    0,
                    0,
                    Some(&progress),
                    &mut output
                )
                .is_err()
        );
        assert!(output.is_empty());
        let event = |index: usize| JudgeEvent {
            object: chart.notes[index].object,
            stage: JudgeStage::Instant,
            outcome: JudgeOutcome::Hit {
                grade: JudgeGrade(1),
                delta: Duration::ZERO,
            },
            at: Timestamp::from_nanos(i64::MAX),
            input: None,
        };
        progress.apply(&[event(0), event(1)]);
        chart
            .visible_note_indices_with_progress_checked(
                Timestamp::ZERO,
                0,
                0,
                Some(&progress),
                &mut output,
            )
            .unwrap();
        assert_eq!(output, (2..MAX_VISIBLE_NOTES + 2).collect::<Vec<_>>());
        let pointer = output.as_ptr();
        let capacity = output.capacity();
        chart
            .visible_note_indices_with_progress_checked(
                Timestamp::ZERO,
                0,
                0,
                Some(&progress),
                &mut output,
            )
            .unwrap();
        assert_eq!(output.as_ptr(), pointer);
        assert_eq!(output.capacity(), capacity);
        let foreign = NoteProgress::new(Arc::new(chart.as_ref().clone())).unwrap();
        assert!(
            chart
                .visible_note_indices_with_progress_checked(
                    Timestamp::ZERO,
                    0,
                    0,
                    Some(&foreign),
                    &mut output
                )
                .is_err()
        );
        assert!(output.is_empty());
        assert_eq!(chart.note_index_by_object(chart.notes[2].object), Some(2));
    }

    #[test]
    fn indexed_query_matches_linear_overlap_oracle_and_reuses_warmed_storage() {
        let notes = [
            (i64::MIN, Some(i64::MIN + 1)),
            (-5, Some(10)),
            (-1, None),
            (0, None),
            (5, Some(i64::MAX)),
            (i64::MAX, None),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (start, end))| PlayerNote {
            object: ObjectId(index as u64 + 1),
            lane_index: 0,
            start: Timestamp::from_nanos(start),
            end: end.map(Timestamp::from_nanos),
        })
        .collect();
        let chart = indexed_model(notes);
        let mut output = Vec::new();
        chart
            .visible_note_indices_checked(Timestamp::ZERO, i64::MAX, i64::MAX, &mut output)
            .unwrap();
        let pointer = output.as_ptr();
        let capacity = output.capacity();
        for now in [
            i64::MIN,
            i64::MIN + 1,
            -1,
            0,
            5,
            10,
            i64::MAX - 1,
            i64::MAX,
            5,
            0,
        ] {
            for lookahead in [0, 10, i64::MAX] {
                for behind in [0, 10, i64::MAX] {
                    let lower = i128::from(now) - i128::from(behind);
                    let upper = i128::from(now) + i128::from(lookahead);
                    let expected: Vec<_> = chart
                        .notes
                        .iter()
                        .enumerate()
                        .filter(|(_, note)| {
                            i128::from(note.start.as_nanos()) <= upper
                                && i128::from(note.end.unwrap_or(note.start).as_nanos()) >= lower
                        })
                        .map(|(index, _)| index)
                        .collect();
                    chart
                        .visible_note_indices_checked(
                            Timestamp::from_nanos(now),
                            lookahead,
                            behind,
                            &mut output,
                        )
                        .unwrap();
                    assert_eq!(output, expected);
                    assert_eq!(output.as_ptr(), pointer);
                    assert_eq!(output.capacity(), capacity);
                    let references = chart
                        .visible_notes_checked(Timestamp::from_nanos(now), lookahead, behind)
                        .unwrap();
                    assert_eq!(
                        references
                            .iter()
                            .map(|note| note.object)
                            .collect::<Vec<_>>(),
                        output
                            .iter()
                            .map(|&index| chart.notes[index].object)
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
        for (lookahead, behind) in [(-1, 0), (0, -1)] {
            chart
                .visible_note_indices_checked(Timestamp::ZERO, lookahead, behind, &mut output)
                .unwrap();
            assert!(output.is_empty());
            assert_eq!(output.as_ptr(), pointer);
        }
        let replacement = indexed_model(vec![PlayerNote {
            object: ObjectId(99),
            lane_index: 0,
            start: Timestamp::ZERO,
            end: None,
        }]);
        replacement
            .visible_note_indices_checked(Timestamp::ZERO, 0, 0, &mut output)
            .unwrap();
        assert_eq!(output, vec![0]);
        assert_eq!(output.as_ptr(), pointer);
        assert_eq!(output.capacity(), capacity);
        indexed_model(vec![])
            .visible_note_indices_checked(Timestamp::ZERO, 0, 0, &mut output)
            .unwrap();
        assert!(output.is_empty());
        assert_eq!(output.as_ptr(), pointer);
    }

    #[test]
    fn indexed_budget_is_exact_and_overflow_clears_without_losing_storage() {
        let dense = |count| {
            indexed_model(
                (0..count)
                    .map(|index| PlayerNote {
                        object: ObjectId(index as u64 + 1),
                        lane_index: 0,
                        start: Timestamp::ZERO,
                        end: None,
                    })
                    .collect(),
            )
        };
        let exact = dense(MAX_VISIBLE_NOTES);
        let overflow = dense(MAX_VISIBLE_NOTES + 1);
        let mut output = vec![usize::MAX];
        exact
            .visible_note_indices_checked(Timestamp::ZERO, 0, 0, &mut output)
            .unwrap();
        assert_eq!(output.len(), MAX_VISIBLE_NOTES);
        assert!(
            overflow
                .visible_note_indices_checked(Timestamp::ZERO, 0, 0, &mut output)
                .is_err()
        );
        assert!(output.is_empty());
        let pointer = output.as_ptr();
        let capacity = output.capacity();
        exact
            .visible_note_indices_checked(Timestamp::ZERO, 0, 0, &mut output)
            .unwrap();
        assert_eq!(output.as_ptr(), pointer);
        assert_eq!(output.capacity(), capacity);
        assert!(
            overflow
                .visible_notes_checked(Timestamp::ZERO, 0, 0)
                .is_err()
        );
        assert_eq!(
            overflow
                .visible_notes(Timestamp::ZERO, 0, 0, usize::MAX)
                .len(),
            MAX_VISIBLE_NOTES
        );
    }
    use beatkernel_bms::{ParseOptions, parse};

    fn model(text: &str) -> PlayerChart {
        let source = parse(text, ParseOptions::default()).unwrap();
        PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap()
    }

    #[test]
    fn lnobj_body_uses_compiled_bpm_stop_endpoint_without_endpoint_note() {
        let source = parse(
            "#BPM 60\n#BPM01 120\n#STOP01 48\n#LNOBJ ZZ\n#WAV01 tap.wav\n#00011:010000ZZ\n#00008:00010000\n#00009:00010000\n",
            ParseOptions::default(),
        ).unwrap();
        let compiled = source.compile().unwrap();
        let chart = PlayerChart::from_compiled(&source, &compiled.chart).unwrap();
        assert_eq!(chart.lanes, [0x11]);
        assert_eq!(chart.notes.len(), 1);
        let note = &chart.notes[0];
        assert_eq!(note.object, compiled.chart.objects()[0].id);
        assert_eq!(note.start, Timestamp::ZERO);
        assert_eq!(note.end, Some(Timestamp::from_nanos(2_500_000_000)));
        assert_eq!(chart.duration_ns, 2_500_000_000);
        for at in [0, 1_250_000_000, 2_499_999_999, 2_500_000_000] {
            let visible = chart
                .visible_notes_checked(Timestamp::from_nanos(at), 0, 0)
                .unwrap();
            assert_eq!(visible, [note]);
        }
        assert!(
            chart
                .visible_notes_checked(Timestamp::from_nanos(2_500_000_001), 0, 0)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn unicode_lane_order_and_exact_hold_overlap() {
        let chart = model(
            "#TITLE 별빛\n#ARTIST 作曲家\n#BPM 60\n#LNTYPE 1\n#WAV01 tap.wav\n#00016:01\n#00021:01\n#00026:01\n#00051:0101\n",
        );
        assert_eq!(
            (chart.title.as_str(), chart.artist.as_str()),
            ("별빛", "作曲家")
        );
        assert_eq!(chart.lanes, vec![0x16, 0x11, 0x21, 0x26]);
        let visible = chart.visible_notes(Timestamp::from_nanos(1_000_000_000), 0, 0, 10);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].start, Timestamp::ZERO);
        assert_eq!(visible[0].end, Some(Timestamp::from_nanos(2_000_000_000)));
        assert!(
            chart
                .visible_notes(Timestamp::from_nanos(2_000_000_001), 0, 0, 10)
                .is_empty()
        );
    }

    #[test]
    fn wide_windows_and_frame_cap() {
        let chart = model("#BPM 60\n#WAV01 tap.wav\n#00011:0101\n");
        assert_eq!(
            chart
                .visible_notes(Timestamp::MAX, i64::MAX, i64::MAX, 10)
                .len(),
            2
        );
        assert!(
            chart
                .visible_notes(Timestamp::MIN, i64::MAX, i64::MAX, 10)
                .is_empty()
        );
        assert_eq!(
            chart.visible_notes(Timestamp::ZERO, i64::MAX, 0, 1).len(),
            1
        );
        assert!(chart.visible_notes(Timestamp::ZERO, -1, 0, 10).is_empty());
        let dense = model(&format!(
            "#BPM 60\n#WAV01 tap.wav\n#00011:{}\n",
            "01".repeat(2400)
        ));
        assert_eq!(
            dense
                .visible_notes(Timestamp::ZERO, i64::MAX, 0, usize::MAX)
                .len(),
            MAX_VISIBLE_NOTES
        );
        assert!(
            dense
                .visible_notes_checked(Timestamp::ZERO, i64::MAX, 0)
                .is_err()
        );
        assert_eq!(
            chart
                .visible_notes_checked(Timestamp::ZERO, i64::MAX, 0)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn mismatched_compiled_mapping_is_rejected() {
        let mut source = parse(
            "#BPM 60\n#WAV01 tap.wav\n#00011:01\n",
            ParseOptions::default(),
        )
        .unwrap();
        let compiled = source.compile().unwrap();
        source.notes[0].object = ObjectId(999);
        assert!(PlayerChart::from_compiled(&source, &compiled.chart).is_err());
    }

    struct TempLibrary(PathBuf);
    impl TempLibrary {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "beatkernel-player-catalog-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempLibrary {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn library_keeps_unicode_sort_and_parse_diagnostics_without_assets() {
        let root = TempLibrary::new();
        fs::create_dir(root.0.join("nested")).unwrap();
        fs::write(
            root.0.join("z.bms"),
            "#TITLE 별빛\n#ARTIST 作曲家\n#BPM 60\n#WAV01 nonexistent.wav\n#00011:01\n",
        )
        .unwrap();
        fs::write(root.0.join("nested/a.BME"), "#TITLE Alpha\n#BPM 60\n").unwrap();
        fs::write(root.0.join("invalid.bms"), [0xff, 0xfe]).unwrap();
        fs::write(root.0.join("ignore.txt"), "not a chart").unwrap();
        let library = scan_library(&root.0).unwrap();
        assert_eq!(library.entries.len(), 2);
        assert_eq!(library.entries[0].title, "Alpha");
        assert_eq!(library.entries[1].artist, "作曲家");
        assert_eq!(library.diagnostics.len(), 1);
        assert!(library.diagnostics[0].contains("invalid.bms"));
        assert!(scan_library(&root.0.join("z.bms")).is_err());
    }

    #[test]
    fn directory_depth_is_finite_and_visible() {
        let root = TempLibrary::new();
        let mut path = root.0.clone();
        for _ in 0..9 {
            path.push("deeper");
            fs::create_dir(&path).unwrap();
        }
        fs::write(path.join("unvisited.bms"), "#BPM 60\n").unwrap();
        let library = scan_library(&root.0).unwrap();
        assert!(library.entries.is_empty());
        assert!(
            library
                .diagnostics
                .iter()
                .any(|message| message.contains("directory/depth limit"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_files_directories_and_roots_are_not_followed() {
        use std::os::unix::fs::symlink;
        let root = TempLibrary::new();
        let outside = TempLibrary::new();
        fs::write(outside.0.join("song.bms"), "#BPM 60\n").unwrap();
        symlink(&outside.0, root.0.join("directory-link")).unwrap();
        symlink(outside.0.join("song.bms"), root.0.join("song.bms")).unwrap();
        assert!(scan_library(&root.0).unwrap().entries.is_empty());
        assert!(scan_library(&root.0.join("directory-link")).is_err());
    }
}
