//! Renderer-local progress import from real compiled identities and judge output.
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

struct Fixture {
    chart: Arc<PlayerChart>,
    misses: Vec<JudgeEvent>,
    head: JudgeEvent,
    tail: JudgeEvent,
}

fn fixture(instant_count: usize) -> Fixture {
    let text = format!(
        "#BPM 60\n#WAV01 key.wav\n#00011:{}\n#00152:0101\n",
        "01".repeat(instant_count)
    );
    let source = beatkernel_bms::parse(&text, Default::default()).unwrap();
    let compiled = source.compile().unwrap();
    let chart = Arc::new(PlayerChart::from_compiled(&source, &compiled.chart).unwrap());
    assert_eq!(chart.notes.len(), instant_count + 1);
    assert_eq!(
        chart
            .notes
            .iter()
            .map(|note| note.object)
            .collect::<BTreeSet<_>>()
            .len(),
        chart.notes.len()
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
    let hold = chart.notes.last().unwrap();
    let misses = judge.advance_to(hold.start).unwrap();
    assert_eq!(misses.len(), instant_count);
    assert!(misses.iter().all(|event| event.stage == JudgeStage::Instant
        && matches!(event.outcome, JudgeOutcome::Miss { .. })));
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
    Fixture {
        chart,
        misses,
        head: heads[0],
        tail: tails[0],
    }
}

fn miss(f: &Fixture, index: usize) -> JudgeEvent {
    *f.misses
        .iter()
        .find(|event| event.object == f.chart.notes[index].object)
        .unwrap()
}

#[derive(Clone)]
struct WirePage {
    index: usize,
    valid: usize,
    completed: usize,
    words: Vec<u64>,
}

fn wire(current: &NoteProgress, baseline: &NoteProgress) -> Vec<WirePage> {
    current
        .changed_pages_since(baseline)
        .unwrap()
        .map(|page| WirePage {
            index: page.index(),
            valid: page.valid_count(),
            completed: page.completed_count(),
            words: page.packed_states().to_vec(),
        })
        .collect()
}

fn import(
    receiver: &mut NoteProgress,
    pages: &[WirePage],
    last_miss: Option<Timestamp>,
) -> Result<(), String> {
    let updates: Vec<_> = pages
        .iter()
        .map(|page| NoteProgressPageUpdate {
            index: page.index,
            valid_count: page.valid,
            completed_count: page.completed,
            packed_states: &page.words,
        })
        .collect();
    receiver.apply_page_updates(&updates, last_miss)
}

fn receiver(f: &Fixture) -> NoteProgress {
    // A distinct process reconstructs equal visual content in its own allocation.
    let chart = Arc::new(f.chart.as_ref().clone());
    assert!(!Arc::ptr_eq(&chart, &f.chart));
    assert_eq!(chart.notes, f.chart.notes);
    NoteProgress::new(chart).unwrap()
}

fn assert_unchanged(actual: &NoteProgress, before: &NoteProgress) {
    assert!(Arc::ptr_eq(&actual.chart, &before.chart));
    assert!(Arc::ptr_eq(&actual.pages, &before.pages));
    assert_eq!(actual.last_miss(), before.last_miss());
    for (page, old) in actual.pages.iter().zip(before.pages.iter()) {
        assert!(Arc::ptr_eq(page, old));
        assert_eq!(page.bits, old.bits);
        assert_eq!(page.completed, old.completed);
    }
    for index in 0..actual.chart.notes.len() {
        assert_eq!(actual.state(index), before.state(index));
    }
}

#[test]
fn renderer_local_import_coalesces_skipped_ack_pages_and_preserves_producer_identity_guard() {
    let f = fixture(8193);
    let mut producer = NoteProgress::new(f.chart.clone()).unwrap();
    let acknowledged = producer.clone();
    let mut renderer = receiver(&f);
    let pristine = renderer.clone();
    assert!(producer.changed_pages_since(&renderer).is_err());
    assert!(renderer.matches_chart(&renderer.chart));
    assert!(!renderer.matches_chart(&f.chart));
    producer.apply(&[miss(&f, 4096)]);
    let skipped = producer.clone();
    producer.apply(&[miss(&f, 0), miss(&f, 8192), f.head]);
    let pages = wire(&producer, &acknowledged);
    assert_eq!(
        pages.iter().map(|page| page.index).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(
        pages.iter().map(|page| page.valid).collect::<Vec<_>>(),
        [4096, 4096, 2]
    );
    import(&mut renderer, &pages, producer.last_miss()).unwrap();
    for index in 0..f.chart.notes.len() {
        assert_eq!(renderer.state(index), producer.state(index));
        assert_eq!(pristine.state(index), Some(NoteState::Pending));
    }
    assert_eq!(renderer.last_miss(), producer.last_miss());
    assert_eq!(skipped.state(0), Some(NoteState::Pending));
    assert_eq!(acknowledged.state(4096), Some(NoteState::Pending));
    let holding = renderer.clone();
    let fully_acked = producer.clone();
    producer.apply(&[f.tail]);
    let tail_pages = wire(&producer, &fully_acked);
    assert_eq!(tail_pages.len(), 1);
    assert_eq!(tail_pages[0].index, 2);
    assert_eq!(tail_pages[0].completed, 2);
    import(&mut renderer, &tail_pages, producer.last_miss()).unwrap();
    assert!(Arc::ptr_eq(&renderer.pages[0], &holding.pages[0]));
    assert!(Arc::ptr_eq(&renderer.pages[1], &holding.pages[1]));
    assert!(!Arc::ptr_eq(&renderer.pages[2], &holding.pages[2]));
    assert_eq!(holding.state(8193), Some(NoteState::Holding));
    assert_eq!(renderer.state(8193), Some(NoteState::Completed));
    assert!(renderer.all_completed(8192, 8194));
    assert!(!renderer.all_completed(0, 8194));
}

#[test]
fn unchanged_page_payloads_and_empty_updates_reuse_every_arc() {
    let f = fixture(8193);
    let mut producer = NoteProgress::new(f.chart.clone()).unwrap();
    let base = producer.clone();
    producer.apply(&[miss(&f, 0), f.head]);
    let pages = wire(&producer, &base);
    let mut renderer = receiver(&f);
    let zero = renderer.clone();
    import(&mut renderer, &pages, producer.last_miss()).unwrap();
    assert!(Arc::ptr_eq(&renderer.pages[1], &zero.pages[1]));
    assert!(!Arc::ptr_eq(&renderer.pages[0], &zero.pages[0]));
    let before = renderer.clone();
    import(&mut renderer, &pages, producer.last_miss()).unwrap();
    assert_unchanged(&renderer, &before);
    import(&mut renderer, &[], producer.last_miss()).unwrap();
    assert_unchanged(&renderer, &before);
}

#[test]
fn complete_real_prefix_imports_full_page_counts_and_boundary_ranges() {
    let f = fixture(8193);
    let mut producer = NoteProgress::new(f.chart.clone()).unwrap();
    let pending = producer.clone();
    producer.apply(&f.misses);
    producer.apply(&[f.head, f.tail]);
    let pages = wire(&producer, &pending);
    assert_eq!(
        pages.iter().map(|page| page.completed).collect::<Vec<_>>(),
        [4096, 4096, 2]
    );
    let mut renderer = receiver(&f);
    import(&mut renderer, &pages, producer.last_miss()).unwrap();
    for (first, last) in [
        (0, 8194),
        (0, 4096),
        (4096, 8192),
        (4095, 4097),
        (8192, 8194),
    ] {
        assert!(renderer.all_completed(first, last), "{first}..{last}");
    }
    assert!(!renderer.all_completed(0, 0));
    assert!(!renderer.all_completed(0, 8195));
    assert_eq!(renderer.state(8194), None);
    assert_eq!(pending.last_miss(), None);
    assert_eq!(pending.state(4096), Some(NoteState::Pending));
}

#[test]
fn scalar_only_negative_and_effective_future_misses_keep_page_storage() {
    let f = fixture(1);
    let mut renderer = receiver(&f);
    let zero = renderer.clone();
    import(&mut renderer, &[], Some(Timestamp::from_nanos(-5))).unwrap();
    assert_eq!(renderer.last_miss(), Some(Timestamp::from_nanos(-5)));
    assert!(Arc::ptr_eq(&renderer.pages, &zero.pages));
    import(&mut renderer, &[], Some(Timestamp::from_nanos(i64::MAX))).unwrap();
    assert_eq!(renderer.last_miss(), Some(Timestamp::from_nanos(i64::MAX)));
    assert!(Arc::ptr_eq(&renderer.pages, &zero.pages));
    assert_eq!(zero.last_miss(), None);
    let before = renderer.clone();
    for scalar in [None, Some(Timestamp::from_nanos(-6))] {
        assert!(import(&mut renderer, &[], scalar).is_err());
        assert_unchanged(&renderer, &before);
    }
}

#[test]
fn empty_compiled_chart_accepts_scalar_but_rejects_any_page() {
    let source = beatkernel_bms::parse("#BPM 120\n", Default::default()).unwrap();
    let chart =
        Arc::new(PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap());
    let mut renderer = NoteProgress::new(chart).unwrap();
    import(&mut renderer, &[], Some(Timestamp::from_nanos(-1))).unwrap();
    assert!(renderer.pages.is_empty());
    assert_eq!(renderer.state(0), None);
    let before = renderer.clone();
    let page = WirePage {
        index: 0,
        valid: 0,
        completed: 0,
        words: vec![0; 128],
    };
    assert!(import(&mut renderer, &[page], Some(Timestamp::from_nanos(99))).is_err());
    assert_unchanged(&renderer, &before);
}

#[test]
fn malformed_partial_page_shapes_states_padding_and_counts_are_atomic() {
    let f = fixture(33);
    let mut producer = NoteProgress::new(f.chart.clone()).unwrap();
    let base = producer.clone();
    producer.apply(&[miss(&f, 0), f.head]);
    let valid = wire(&producer, &base).remove(0);
    assert_eq!(valid.valid, 34);
    assert_eq!(valid.completed, 1);
    let mut malformed = Vec::new();
    for extent in [0, 33, 35, 4097, usize::MAX] {
        let mut page = valid.clone();
        page.valid = extent;
        malformed.push(page);
    }
    for count in [0, 2, 35, usize::MAX] {
        let mut page = valid.clone();
        page.completed = count;
        malformed.push(page);
    }
    for length in [0, 127, 129] {
        let mut page = valid.clone();
        page.words.resize(length, 0);
        malformed.push(page);
    }
    let mut reserved = valid.clone();
    reserved.words[0] |= 3 << 2;
    malformed.push(reserved);
    let mut instant_hold = valid.clone();
    instant_hold.words[0] |= 1 << 2;
    malformed.push(instant_hold);
    let mut padding = valid.clone();
    padding.words[1] |= 2 << 4;
    malformed.push(padding);
    let mut far_padding = valid.clone();
    far_padding.words[127] = 1 << 62;
    malformed.push(far_padding);
    for index in [1, usize::MAX] {
        let mut page = valid.clone();
        page.index = index;
        malformed.push(page);
    }
    let mut renderer = receiver(&f);
    let before = renderer.clone();
    for (case, page) in malformed.into_iter().enumerate() {
        assert!(
            import(
                &mut renderer,
                &[page],
                Some(Timestamp::from_nanos(i64::MAX))
            )
            .is_err(),
            "case {case}"
        );
        assert_unchanged(&renderer, &before);
    }
    import(&mut renderer, &[valid], producer.last_miss()).unwrap();
    assert_eq!(renderer.state(33), Some(NoteState::Holding));
    assert_eq!(renderer.pages[0].completed, 1);
    assert_eq!(renderer.pages[0].bits[1], 1 << 2);
    assert!(renderer.pages[0].bits[2..].iter().all(|word| *word == 0));
}

#[test]
fn duplicate_descending_and_unknown_final_page_reject_whole_batch() {
    let f = fixture(8193);
    let mut producer = NoteProgress::new(f.chart.clone()).unwrap();
    let base = producer.clone();
    producer.apply(&[miss(&f, 0), miss(&f, 4096), f.head]);
    let pages = wire(&producer, &base);
    let mut unknown = pages.clone();
    unknown.last_mut().unwrap().index = 3;
    let batches = [
        vec![pages[0].clone(), pages[0].clone()],
        vec![pages[1].clone(), pages[0].clone()],
        unknown,
    ];
    let mut renderer = receiver(&f);
    let before = renderer.clone();
    for batch in batches {
        assert!(import(&mut renderer, &batch, producer.last_miss()).is_err());
        assert_unchanged(&renderer, &before);
    }
}

#[test]
fn completed_and_holding_states_cannot_regress_even_with_consistent_counts() {
    let f = fixture(1);
    let mut producer = NoteProgress::new(f.chart.clone()).unwrap();
    let pending = producer.clone();
    producer.apply(&[f.head]);
    let hold_page = wire(&producer, &pending);
    let mut renderer = receiver(&f);
    import(&mut renderer, &hold_page, None).unwrap();
    let before = renderer.clone();
    let zero = WirePage {
        index: 0,
        valid: 2,
        completed: 0,
        words: vec![0; 128],
    };
    assert!(import(&mut renderer, &[zero.clone()], None).is_err());
    assert_unchanged(&renderer, &before);
    producer.apply(&[f.tail, miss(&f, 0)]);
    import(
        &mut renderer,
        &wire(&producer, &pending),
        producer.last_miss(),
    )
    .unwrap();
    let completed = renderer.clone();
    for regression in [zero, hold_page[0].clone()] {
        assert!(import(&mut renderer, &[regression], producer.last_miss()).is_err());
        assert_unchanged(&renderer, &completed);
    }
    assert_eq!(before.state(1), Some(NoteState::Holding));
    assert!(renderer.all_completed(0, 2));
}

#[test]
fn bad_final_page_or_scalar_never_publishes_earlier_valid_pages() {
    let f = fixture(8193);
    let mut producer = NoteProgress::new(f.chart.clone()).unwrap();
    let pending = producer.clone();
    producer.apply(&[miss(&f, 0)]);
    let mut renderer = receiver(&f);
    import(
        &mut renderer,
        &wire(&producer, &pending),
        producer.last_miss(),
    )
    .unwrap();
    let acknowledged = producer.clone();
    let retained = renderer.clone();
    producer.apply(&[miss(&f, 1), miss(&f, 4096), f.head]);
    let valid = wire(&producer, &acknowledged);
    assert_eq!(valid.len(), 3);
    let mut bad_final = valid.clone();
    bad_final.last_mut().unwrap().words[127] = 1;
    assert!(
        import(
            &mut renderer,
            &bad_final,
            Some(Timestamp::from_nanos(i64::MAX))
        )
        .is_err()
    );
    assert_unchanged(&renderer, &retained);
    for scalar in [None, Some(Timestamp::from_nanos(i64::MIN))] {
        assert!(import(&mut renderer, &valid, scalar).is_err());
        assert_unchanged(&renderer, &retained);
    }
    import(&mut renderer, &valid, producer.last_miss()).unwrap();
    assert_eq!(renderer.state(1), Some(NoteState::Completed));
    assert_eq!(renderer.state(4096), Some(NoteState::Completed));
    assert_eq!(renderer.state(8193), Some(NoteState::Holding));
    assert_eq!(retained.state(1), Some(NoteState::Pending));
    assert_eq!(retained.state(4096), Some(NoteState::Pending));
    assert_eq!(retained.state(8193), Some(NoteState::Pending));
}
