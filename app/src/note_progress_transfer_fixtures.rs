//! Retained producer views tested with compiled identities and admitted judge events.
use super::*;
use beatkernel::{
    input::{
        ButtonEvent, ButtonState, DeviceId, EventMeta, GameInputEvent, PhysicalControlId,
        PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    time::{ClockDomainId, ClockPoint, Duration},
};
use std::collections::BTreeSet;

struct PreparedEvents {
    chart: Arc<PlayerChart>,
    instants: Vec<JudgeEvent>,
    head: JudgeEvent,
    tail: JudgeEvent,
}

fn prepared_events() -> PreparedEvents {
    // One real dense BMS channel crosses two complete pages; the final page
    // contains one instant and one hold. Parsing assigns every object its ID.
    let text = format!(
        "#BPM 60\n#WAV01 key.wav\n#00011:{}\n#00152:0101\n",
        "01".repeat(8193)
    );
    let source = beatkernel_bms::parse(&text, Default::default()).unwrap();
    let compiled = source.compile().unwrap();
    let chart = Arc::new(PlayerChart::from_compiled(&source, &compiled.chart).unwrap());
    assert_eq!(chart.notes.len(), 8194);
    assert_eq!(
        chart
            .notes
            .iter()
            .map(|n| n.object)
            .collect::<BTreeSet<_>>()
            .len(),
        8194
    );
    let mut judge = JudgeEngine::new(
        compiled.chart,
        source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    let hold = &chart.notes[8193];
    assert!(hold.end.is_some());
    let instants = judge.advance_to(hold.start).unwrap();
    assert_eq!(instants.len(), 8193);
    assert!(
        instants
            .iter()
            .all(|event| event.stage == JudgeStage::Instant)
    );
    let key = GameInputEvent {
        game_control: source
            .notes
            .iter()
            .find(|note| note.object == hold.object)
            .unwrap()
            .lane
            .control(),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(
                DeviceId(1),
                ClockPoint {
                    domain: ClockDomainId(1),
                    timestamp: hold.start,
                },
                1,
            ),
            control: PhysicalControlId::keyboard(4),
            state: ButtonState::Down,
        }),
    };
    let heads = judge.push_input(&key, hold.start).unwrap();
    assert_eq!(heads.len(), 1);
    assert_eq!(heads[0].stage, JudgeStage::HoldHead);
    assert!(matches!(heads[0].outcome, JudgeOutcome::Hit { .. }));
    // The real hold evaluator completes on its owner's Up. Advancing exactly
    // to the inclusive endpoint does not expire an otherwise held note.
    let end = hold.end.unwrap();
    let mut release = key.clone();
    let PhysicalInputEvent::Button(button) = &mut release.physical else {
        unreachable!()
    };
    button.meta = EventMeta::new(
        DeviceId(1),
        ClockPoint {
            domain: ClockDomainId(1),
            timestamp: end,
        },
        2,
    );
    button.state = ButtonState::Up;
    let tails = judge.push_input(&release, end).unwrap();
    assert_eq!(tails.len(), 1);
    assert_eq!(tails[0].stage, JudgeStage::HoldTail);
    assert!(matches!(tails[0].outcome, JudgeOutcome::Hit { .. }));
    PreparedEvents {
        chart,
        instants,
        head: heads[0],
        tail: tails[0],
    }
}

fn instant(fixture: &PreparedEvents, index: usize) -> JudgeEvent {
    *fixture
        .instants
        .iter()
        .find(|event| event.object == fixture.chart.notes[index].object)
        .unwrap()
}

fn packed(page: &NoteProgressPage<'_>, slot: usize) -> u64 {
    (page.packed_states()[slot / 32] >> ((slot % 32) * 2)) & 3
}

#[test]
fn retained_ack_coalesces_ordered_pages_across_skipped_frames() {
    let f = prepared_events();
    let mut current = NoteProgress::new(f.chart.clone()).unwrap();
    let acknowledged = current.clone();
    current.apply(&[instant(&f, 4096)]);
    let skipped_frame = current.clone();
    current.apply(&[instant(&f, 0), instant(&f, 8192), f.head]);
    let pages: Vec<_> = current
        .changed_pages_since(&acknowledged)
        .unwrap()
        .collect();
    assert_eq!(
        pages
            .iter()
            .map(NoteProgressPage::index)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(
        pages
            .iter()
            .map(NoteProgressPage::valid_count)
            .collect::<Vec<_>>(),
        [4096, 4096, 2]
    );
    assert_eq!(
        pages
            .iter()
            .map(NoteProgressPage::completed_count)
            .collect::<Vec<_>>(),
        [1, 1, 1]
    );
    assert_eq!(packed(&pages[0], 0), 2);
    assert_eq!(packed(&pages[1], 0), 2);
    assert_eq!(packed(&pages[2], 0), 2);
    assert_eq!(packed(&pages[2], 1), 1);
    assert_eq!(
        current
            .changed_pages_since(&skipped_frame)
            .unwrap()
            .map(|p| p.index())
            .collect::<Vec<_>>(),
        [0, 2]
    );
    for index in [0, 4096, 8192, 8193] {
        assert_eq!(acknowledged.state(index), Some(NoteState::Pending));
    }
    assert_eq!(skipped_frame.state(0), Some(NoteState::Pending));
    assert_eq!(skipped_frame.state(4096), Some(NoteState::Completed));
    assert_eq!(acknowledged.last_miss(), None);
    assert!(current.last_miss().is_some());
}

#[test]
fn holding_completion_uses_partial_extent_counts_and_zero_padding() {
    let f = prepared_events();
    let mut current = NoteProgress::new(f.chart.clone()).unwrap();
    let pending = current.clone();
    current.apply(&[f.head]);
    let holding = current.clone();
    let page = holding
        .changed_pages_since(&pending)
        .unwrap()
        .next()
        .unwrap();
    assert_eq!(
        (page.index(), page.valid_count(), page.completed_count()),
        (2, 2, 0)
    );
    assert_eq!(page.packed_states()[0], 1 << 2);
    assert!(page.packed_states()[1..].iter().all(|word| *word == 0));
    current.apply(&[f.tail, instant(&f, 8192)]);
    let page = current
        .changed_pages_since(&holding)
        .unwrap()
        .next()
        .unwrap();
    assert_eq!(
        (page.index(), page.valid_count(), page.completed_count()),
        (2, 2, 2)
    );
    assert_eq!(page.packed_states()[0], 2 | (2 << 2));
    assert!(page.packed_states()[1..].iter().all(|word| *word == 0));
    assert_eq!(holding.state(8193), Some(NoteState::Holding));
    assert_eq!(pending.state(8193), Some(NoteState::Pending));
    assert!(current.all_completed(8192, 8194));
    assert!(!current.all_completed(0, 8194));
}

#[test]
fn unchanged_and_duplicate_prefixes_keep_exhausted_borrowed_directory() {
    let f = prepared_events();
    let mut current = NoteProgress::new(f.chart.clone()).unwrap();
    current.apply(&[instant(&f, 0), f.head, f.tail]);
    let acknowledged = current.clone();
    current.apply(&[]);
    current.apply(&[instant(&f, 0), f.head, f.tail]);
    assert!(Arc::ptr_eq(&current.pages, &acknowledged.pages));
    let directory_refs = Arc::strong_count(&current.pages);
    let chart_refs = Arc::strong_count(&f.chart);
    let mut changes = current.changed_pages_since(&acknowledged).unwrap();
    assert!(changes.next().is_none());
    assert!(changes.next().is_none());
    assert_eq!(Arc::strong_count(&current.pages), directory_refs);
    assert_eq!(Arc::strong_count(&f.chart), chart_refs);
    // Page deltas do not replace the scalar presentation update.
    assert!(current.last_miss().is_some());
    assert_eq!(current.last_miss(), acknowledged.last_miss());
}

#[test]
fn changed_pages_borrow_the_current_packed_storage_without_arc_clones() {
    let f = prepared_events();
    let mut current = NoteProgress::new(f.chart.clone()).unwrap();
    let acknowledged = current.clone();
    current.apply(&[instant(&f, 0), instant(&f, 4096), f.head]);
    let directory_refs = Arc::strong_count(&current.pages);
    let chart_refs = Arc::strong_count(&f.chart);
    let page_refs: Vec<_> = current.pages.iter().map(Arc::strong_count).collect();
    let mut changes = current.changed_pages_since(&acknowledged).unwrap();
    for index in 0..3 {
        let page = changes.next().unwrap();
        assert_eq!(page.index(), index);
        assert!(std::ptr::eq(
            page.packed_states(),
            &current.pages[index].bits
        ));
        assert!(std::ptr::eq(page.packed_states(), page.packed_states()));
    }
    assert!(changes.next().is_none());
    assert_eq!(Arc::strong_count(&current.pages), directory_refs);
    assert_eq!(Arc::strong_count(&f.chart), chart_refs);
    assert_eq!(
        current
            .pages
            .iter()
            .map(Arc::strong_count)
            .collect::<Vec<_>>(),
        page_refs
    );
    assert!(
        acknowledged
            .pages
            .iter()
            .all(|page| page.bits.iter().all(|word| *word == 0))
    );
}

#[test]
fn exact_chart_allocation_guard_rejects_equal_content_and_preserves_both_snapshots() {
    let f = prepared_events();
    let mut current = NoteProgress::new(f.chart.clone()).unwrap();
    current.apply(&[f.head]);
    let other_chart = Arc::new(f.chart.as_ref().clone());
    assert_eq!(other_chart.notes, f.chart.notes);
    let other = NoteProgress::new(other_chart).unwrap();
    assert!(current.changed_pages_since(&other).is_err());
    assert!(other.changed_pages_since(&current).is_err());
    assert_eq!(current.state(8193), Some(NoteState::Holding));
    assert_eq!(other.state(8193), Some(NoteState::Pending));
    assert_eq!(current.last_miss(), None);
    assert_eq!(other.last_miss(), None);
}

#[test]
fn empty_compiled_chart_has_no_page_and_still_enforces_exact_identity() {
    let source = beatkernel_bms::parse("#BPM 120\n", Default::default()).unwrap();
    let chart =
        Arc::new(PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap());
    let current = NoteProgress::new(chart.clone()).unwrap();
    let acknowledged = current.clone();
    assert!(
        current
            .changed_pages_since(&acknowledged)
            .unwrap()
            .next()
            .is_none()
    );
    assert_eq!(current.state(0), None);
    assert_eq!(current.last_miss(), None);
    let other = NoteProgress::new(Arc::new(chart.as_ref().clone())).unwrap();
    assert!(current.changed_pages_since(&other).is_err());
}
