//! Prepared-chart-local object progress from complete admitted judge prefixes.
use crate::player_chart::PlayerChart;
use beatkernel::{
    chart::MAX_SOURCE_ITEMS,
    judge::{JudgeEvent, JudgeOutcome, JudgeStage},
    time::Timestamp,
};
use std::sync::Arc;

const NOTES_PER_PAGE: usize = 4096;
#[derive(Clone, Debug)]
struct Page {
    bits: [u64; NOTES_PER_PAGE / 32],
    completed: u16,
}
/// Monotonic presentation state of one prepared object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteState {
    Pending,
    Holding,
    Completed,
}
/// Immutable snapshots share their directory and two-bit pages. Only real state
/// changes copy the directory and affected pages; empty/ignored prefixes do not.
#[derive(Clone, Debug)]
pub struct NoteProgress {
    chart: Arc<PlayerChart>,
    pages: Arc<Vec<Arc<Page>>>,
    last_miss: Option<Timestamp>,
}
impl NoteProgress {
    /// Prepares bounded zero pages for this exact chart allocation.
    pub fn new(chart: Arc<PlayerChart>) -> Result<Self, String> {
        if chart.notes.len() > MAX_SOURCE_ITEMS {
            return Err("note progress exceeds source item capacity".into());
        }
        let count = chart.notes.len().div_ceil(NOTES_PER_PAGE);
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(count)
            .map_err(|_| "note progress allocation failed")?;
        if count != 0 {
            let zero = Arc::new(Page {
                bits: [0; NOTES_PER_PAGE / 32],
                completed: 0,
            });
            pages.resize(count, zero);
        }
        Ok(Self {
            chart,
            pages: Arc::new(pages),
            last_miss: None,
        })
    }
    /// Applies accepted events immediately, independently of calibrated event time.
    /// Unknown identities and shape-mismatched/custom stages leave state untouched.
    pub fn apply(&mut self, events: &[JudgeEvent]) {
        for event in events {
            let Some(index) = self.chart.note_index_by_object(event.object) else {
                continue;
            };
            let note = &self.chart.notes[index];
            let next = match event.stage {
                JudgeStage::Instant if note.end.is_none() => NoteState::Completed,
                JudgeStage::HoldHead if note.end.is_some() => match event.outcome {
                    JudgeOutcome::Hit { .. } => NoteState::Holding,
                    JudgeOutcome::Miss { .. } => NoteState::Completed,
                },
                JudgeStage::HoldTail if note.end.is_some() => NoteState::Completed,
                _ => continue,
            };
            let Some(old) = self.state(index) else {
                continue;
            };
            if old == next || old == NoteState::Completed {
                continue;
            }
            let page = Arc::make_mut(&mut Arc::make_mut(&mut self.pages)[index / NOTES_PER_PAGE]);
            let slot = (index % NOTES_PER_PAGE) / 32;
            let shift = (index % 32) * 2;
            let value = if next == NoteState::Holding { 1 } else { 2 };
            page.bits[slot] = (page.bits[slot] & !(3 << shift)) | (value << shift);
            if next == NoteState::Completed {
                page.completed += 1;
            }
            if matches!(event.outcome, JudgeOutcome::Miss { .. }) {
                self.last_miss = Some(self.last_miss.map_or(event.at, |old| old.max(event.at)));
            }
        }
    }
    /// Latest effective timestamp of a new accepted, known-stage miss transition.
    /// Hits never clear it; duplicate/completed objects cannot refresh it.
    pub const fn last_miss(&self) -> Option<Timestamp> {
        self.last_miss
    }
    /// Returns state by prepared note index, or None outside that chart.
    pub fn state(&self, index: usize) -> Option<NoteState> {
        if index >= self.chart.notes.len() {
            return None;
        }
        let page = self.pages.get(index / NOTES_PER_PAGE)?;
        match (page.bits[(index % NOTES_PER_PAGE) / 32] >> ((index % 32) * 2)) & 3 {
            0 => Some(NoteState::Pending),
            1 => Some(NoteState::Holding),
            _ => Some(NoteState::Completed),
        }
    }
    /// Whether every note in the exact nonempty half-open range is completed.
    /// Whole pages use counts; boundary fragments inspect packed high bits.
    /// Invalid ranges return false without changing snapshots.
    pub fn all_completed(&self, first: usize, last: usize) -> bool {
        if first >= last || last > self.chart.notes.len() {
            return false;
        }
        let mut current = first;
        while current < last {
            let page_index = current / NOTES_PER_PAGE;
            let page_start = page_index * NOTES_PER_PAGE;
            let valid_end = (page_start + NOTES_PER_PAGE).min(self.chart.notes.len());
            let stop = last.min(valid_end);
            let Some(page) = self.pages.get(page_index) else {
                return false;
            };
            if current == page_start && stop == valid_end {
                if usize::from(page.completed) != valid_end - page_start {
                    return false;
                }
            } else {
                let mut offset = current - page_start;
                let end = stop - page_start;
                while offset < end {
                    let count = (32 - offset % 32).min(end - offset);
                    let low = if count == 32 {
                        u64::MAX
                    } else {
                        (1u64 << (count * 2)) - 1
                    };
                    let mask = (low & 0xaaaa_aaaa_aaaa_aaaa) << ((offset % 32) * 2);
                    if page.bits[offset / 32] & mask != mask {
                        return false;
                    }
                    offset += count;
                }
            }
            current = stop;
        }
        true
    }
    /// Requires the exact prepared allocation, rather than equivalent chart data.
    pub fn matches_chart(&self, chart: &PlayerChart) -> bool {
        std::ptr::eq(self.chart.as_ref(), chart)
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::{
        chart::ObjectId,
        judge::{JudgeGrade, MissReason},
        time::{Duration, Timestamp},
    };
    fn chart() -> Arc<PlayerChart> {
        let source = beatkernel_bms::parse(
            "#BPM 60\n#WAV01 head.wav\n#00051:0101\n#00016:01",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        Arc::new(PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap())
    }
    fn event(object: ObjectId, stage: JudgeStage, miss: bool) -> JudgeEvent {
        JudgeEvent {
            object,
            stage,
            outcome: if miss {
                JudgeOutcome::Miss {
                    reason: MissReason::HeadTimeout,
                }
            } else {
                JudgeOutcome::Hit {
                    grade: JudgeGrade(1),
                    delta: Duration::ZERO,
                }
            },
            at: Timestamp::from_nanos(i64::MAX),
            input: None,
        }
    }
    #[test]
    fn monotonic_stage_shape_and_snapshot_copy_on_write() {
        let chart = chart();
        let hold = chart.notes.iter().position(|n| n.end.is_some()).unwrap();
        let instant = chart.notes.iter().position(|n| n.end.is_none()).unwrap();
        let mut state = NoteProgress::new(chart.clone()).unwrap();
        let original = state.clone();
        state.apply(&[]);
        state.apply(&[
            event(ObjectId(u64::MAX), JudgeStage::Instant, false),
            event(chart.notes[hold].object, JudgeStage::Instant, false),
            event(chart.notes[instant].object, JudgeStage::HoldTail, false),
            event(chart.notes[hold].object, JudgeStage::Custom(1), false),
        ]);
        assert!(Arc::ptr_eq(&state.pages, &original.pages));
        let head = event(chart.notes[hold].object, JudgeStage::HoldHead, false);
        state.apply(&[head]);
        assert_eq!(state.state(hold), Some(NoteState::Holding));
        assert_eq!(original.state(hold), Some(NoteState::Pending));
        let holding = state.clone();
        state.apply(&[head]);
        assert!(Arc::ptr_eq(&state.pages, &holding.pages));
        state.apply(&[
            event(chart.notes[hold].object, JudgeStage::HoldTail, true),
            head,
            event(chart.notes[instant].object, JudgeStage::Instant, true),
        ]);
        assert_eq!(state.state(hold), Some(NoteState::Completed));
        assert_eq!(state.state(instant), Some(NoteState::Completed));
        assert_eq!(holding.state(hold), Some(NoteState::Holding));
        let mut timeout = NoteProgress::new(chart.clone()).unwrap();
        timeout.apply(&[event(chart.notes[hold].object, JudgeStage::HoldHead, true)]);
        assert_eq!(timeout.state(hold), Some(NoteState::Completed));
        assert!(state.matches_chart(&chart));
        assert!(!state.matches_chart(&chart.as_ref().clone()));
        assert_eq!(state.state(chart.notes.len()), None);
    }
    #[test]
    fn pages_share_zero_and_only_changed_page_is_copied() {
        let base = chart();
        let mut large = base.as_ref().clone();
        let template = large.notes[0].clone();
        large.notes.resize(NOTES_PER_PAGE + 1, template);
        let large = Arc::new(large);
        let mut state = NoteProgress::new(large.clone()).unwrap();
        assert!(Arc::ptr_eq(&state.pages[0], &state.pages[1]));
        let old = state.clone();
        let object = large.notes[0].object;
        let stage = if large.notes[0].end.is_some() {
            JudgeStage::HoldHead
        } else {
            JudgeStage::Instant
        };
        state.apply(&[event(object, stage, false)]);
        assert!(!Arc::ptr_eq(&state.pages[0], &old.pages[0]));
        assert!(Arc::ptr_eq(&state.pages[1], &old.pages[1]));
        let mut excessive = base.as_ref().clone();
        excessive
            .notes
            .resize(MAX_SOURCE_ITEMS + 1, base.notes[0].clone());
        assert!(NoteProgress::new(Arc::new(excessive)).is_err());
    }
    #[test]
    fn completed_counts_are_idempotent_and_partial_ranges_match_state_oracle() {
        let chart = chart();
        let mut state = NoteProgress::new(chart.clone()).unwrap();
        let old = state.clone();
        let events: Vec<_> = chart
            .notes
            .iter()
            .map(|note| {
                event(
                    note.object,
                    if note.end.is_some() {
                        JudgeStage::HoldTail
                    } else {
                        JudgeStage::Instant
                    },
                    false,
                )
            })
            .collect();
        state.apply(&events);
        state.apply(&events);
        assert_eq!(state.pages[0].completed, chart.notes.len() as u16);
        assert!(state.all_completed(0, chart.notes.len()));
        assert!(!old.all_completed(0, chart.notes.len()));
        assert!(!state.all_completed(0, 0));
        assert!(!state.all_completed(1, 0));
        assert!(!state.all_completed(0, usize::MAX));
        let mut wide = chart.as_ref().clone();
        wide.notes
            .resize(NOTES_PER_PAGE * 2 + 17, chart.notes[0].clone());
        let mut wide = NoteProgress::new(Arc::new(wide)).unwrap();
        let pages = Arc::make_mut(&mut wide.pages);
        for (page_index, page) in pages.iter_mut().enumerate() {
            let page = Arc::make_mut(page);
            for offset in
                0..NOTES_PER_PAGE.min(NOTES_PER_PAGE * 2 + 17 - page_index * NOTES_PER_PAGE)
            {
                let index = page_index * NOTES_PER_PAGE + offset;
                if index != NOTES_PER_PAGE + 7 {
                    page.bits[offset / 32] |= 2u64 << ((offset % 32) * 2);
                    page.completed += 1;
                }
            }
        }
        for first in [0, 1, 31, 32, 4095, 4096, 4103, 4104, 8192, 8208] {
            for last in [first + 1, 4096, 4103, 4104, 8192, 8209] {
                let expected = first < last
                    && last <= 8209
                    && (first..last).all(|index| wide.state(index) == Some(NoteState::Completed));
                assert_eq!(wide.all_completed(first, last), expected, "{first}..{last}");
            }
        }
        assert!(wide.all_completed(8192, 8209));
        assert_eq!(wide.pages[2].completed, 17);
        assert!(wide.all_completed(0, 4096));
        assert!(!wide.all_completed(0, 8209));
    }
    #[test]
    fn last_miss_tracks_only_new_matching_transitions_with_max_effective_time() {
        let chart = chart();
        let hold = chart
            .notes
            .iter()
            .position(|note| note.end.is_some())
            .unwrap();
        let instant = chart
            .notes
            .iter()
            .position(|note| note.end.is_none())
            .unwrap();
        let mut progress = NoteProgress::new(chart.clone()).unwrap();
        let pristine = progress.clone();
        assert_eq!(progress.last_miss(), None);
        let at = |object, stage, miss, nanos| {
            let mut e = event(object, stage, miss);
            e.at = Timestamp::from_nanos(nanos);
            e
        };
        progress.apply(&[
            at(chart.notes[hold].object, JudgeStage::Instant, true, 900),
            at(chart.notes[instant].object, JudgeStage::HoldTail, true, 900),
            at(chart.notes[hold].object, JudgeStage::Custom(1), true, 900),
            at(ObjectId(u64::MAX), JudgeStage::Instant, true, 900),
        ]);
        assert_eq!(progress.last_miss(), None);
        assert!(Arc::ptr_eq(&progress.pages, &pristine.pages));
        progress.apply(&[at(chart.notes[hold].object, JudgeStage::HoldHead, false, 1)]);
        assert_eq!(progress.last_miss(), None);
        progress.apply(&[at(
            chart.notes[hold].object,
            JudgeStage::HoldTail,
            true,
            200,
        )]);
        let snapshot = progress.clone();
        progress.apply(&[at(
            chart.notes[instant].object,
            JudgeStage::Instant,
            true,
            100,
        )]);
        assert_eq!(progress.last_miss(), Some(Timestamp::from_nanos(200)));
        assert_eq!(pristine.last_miss(), None);
        let completed = progress.clone();
        progress.apply(&[
            at(chart.notes[instant].object, JudgeStage::Instant, true, 999),
            at(chart.notes[hold].object, JudgeStage::HoldTail, true, 999),
            at(
                chart.notes[instant].object,
                JudgeStage::Instant,
                false,
                1000,
            ),
        ]);
        assert_eq!(progress.last_miss(), Some(Timestamp::from_nanos(200)));
        assert!(Arc::ptr_eq(&progress.pages, &completed.pages));
        assert_eq!(snapshot.state(instant), Some(NoteState::Pending));
        let mut head_miss = NoteProgress::new(chart.clone()).unwrap();
        head_miss.apply(&[at(chart.notes[hold].object, JudgeStage::HoldHead, true, -5)]);
        assert_eq!(head_miss.last_miss(), Some(Timestamp::from_nanos(-5)));
        assert_eq!(head_miss.state(hold), Some(NoteState::Completed));
    }
    #[test]
    fn miss_timestamp_retains_full_dense_prefix_and_hits_do_not_clear_it() {
        let text = format!("#BPM 60\n#WAV01 head.wav\n#00011:{}", "01".repeat(140));
        let source = beatkernel_bms::parse(&text, beatkernel_bms::ParseOptions::default()).unwrap();
        let chart = Arc::new(
            PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap(),
        );
        let mut progress = NoteProgress::new(chart.clone()).unwrap();
        let events: Vec<_> = chart.notes[..139]
            .iter()
            .enumerate()
            .map(|(index, note)| {
                let mut e = event(note.object, JudgeStage::Instant, true);
                e.at = Timestamp::from_nanos(138 - index as i64);
                e
            })
            .collect();
        progress.apply(&events);
        assert_eq!(progress.last_miss(), Some(Timestamp::from_nanos(138)));
        assert!(progress.all_completed(0, 139));
        let snapshot = progress.clone();
        progress.apply(&[event(chart.notes[139].object, JudgeStage::Instant, false)]);
        assert_eq!(progress.last_miss(), snapshot.last_miss());
        assert_eq!(snapshot.state(139), Some(NoteState::Pending));
        let fresh = NoteProgress::new(chart).unwrap();
        assert_eq!(fresh.last_miss(), None);
    }
}
