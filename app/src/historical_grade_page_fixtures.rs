//! Deferred exact historical grade paging, with no renderer or live result proof.
use super::*;
use crate::{
    result_archive::{ArchivedResult, ArchivedScore},
    timing::TimingRecord,
};
fn result() -> crate::record_model::HistoricalRecordValue {
    (
        PlayerId(u32::MAX),
        ArchivedResult {
            scope: PlayResultScope::FullSong,
            outcome: PlayResultOutcome::BelowClearThreshold,
            gauge: *crate::gauge::BmsGauge::default().snapshot(),
        },
    )
}
fn score(count: usize) -> ArchivedScore {
    let grades: Vec<_> = (0..count)
        .map(|index| {
            (
                if index + 1 == count {
                    u32::MAX
                } else {
                    index as u32
                },
                index as u64 + 1,
            )
        })
        .collect();
    ArchivedScore {
        hits: grades.iter().map(|row| row.1).sum(),
        misses: 0,
        combo: 0,
        max_combo: 0,
        grades,
        timing: TimingRecord::default(),
    }
}
fn packet(view: &HistoricalRecordPresentation) -> Vec<([f32; 4], [f32; 4], [f32; 4])> {
    let mut scene = Scene::new(960, 720);
    view.compose(&mut scene).unwrap();
    scene
        .rectangles()
        .iter()
        .map(|rectangle| (rectangle.bounds, rectangle.uv, rectangle.color))
        .collect()
}
fn label(view: &HistoricalRecordPresentation, text: &str) {
    let mut scene = Scene::new(960, 720);
    view.compose(&mut scene).unwrap();
    let expected: Vec<_> = text.chars().map(crate::font::glyph_uv).collect();
    let actual: Vec<_> = scene
        .rectangles()
        .iter()
        .map(|rectangle| rectangle.uv)
        .collect();
    assert!(
        actual
            .windows(expected.len())
            .any(|window| window == expected.as_slice()),
        "missing exact grade label {text}"
    );
}
#[test]
fn unavailable_and_present_empty_are_distinct_single_pages_without_grade_rows() {
    assert_eq!(GRADE_ROWS_PER_PAGE, 4);
    let mut legacy = HistoricalRecordPresentation::from_record(result(), None).unwrap();
    assert_eq!((legacy.grade_page(), legacy.grade_page_count()), (0, 1));
    label(&legacy, "STORED GRADES UNAVAILABLE");
    assert!(!legacy.set_grade_page(0).unwrap());
    assert!(legacy.set_grade_page(1).is_err());
    let mut empty = HistoricalRecordPresentation::from_record(result(), Some(&score(0))).unwrap();
    assert_eq!((empty.grade_page(), empty.grade_page_count()), (0, 1));
    label(&empty, "STORED GRADES EMPTY");
    assert!(!empty.set_grade_page(0).unwrap());
}
#[test]
fn one_four_five_and_maximum_grade_tables_expose_every_sorted_row_on_bounded_pages() {
    for count in [1usize, 4, 5, 4096] {
        let score = score(count);
        let mut view = HistoricalRecordPresentation::from_record(result(), Some(&score)).unwrap();
        assert_eq!(view.grade_page_count(), count.div_ceil(4));
        for page in 0..view.grade_page_count() {
            assert_eq!(view.set_grade_page(page).unwrap(), page != 0);
            assert_eq!(view.grade_page(), page);
            for &(grade, count) in &score.grades[page * 4..((page + 1) * 4).min(score.grades.len())]
            {
                label(&view, &format!("STORED GRADE {grade} COUNT {count}"));
            }
        }
    }
}
#[test]
fn opaque_zero_max_ids_and_counts_above_number_precision_are_literal_exact_integers() {
    let count = 9_007_199_254_740_993u64;
    let score = ArchivedScore {
        hits: u64::MAX,
        misses: u64::MAX,
        combo: u64::MAX,
        max_combo: u64::MAX,
        grades: vec![(0, count), (u32::MAX, u64::MAX - count)],
        timing: TimingRecord::default(),
    };
    let view = HistoricalRecordPresentation::from_record(result(), Some(&score)).unwrap();
    label(&view, "STORED GRADE 0 COUNT 9007199254740993");
    label(
        &view,
        &format!("STORED GRADE 4294967295 COUNT {}", u64::MAX - count),
    );
    assert_eq!(view.score().unwrap(), &score);
}
#[test]
fn paging_retains_grade_vector_and_base_packet_while_only_leaf_rows_change() {
    let mut view = HistoricalRecordPresentation::from_record(result(), Some(&score(5))).unwrap();
    let grade_pointer = view.score().unwrap().grades.as_ptr();
    let first = packet(&view);
    assert!(view.set_grade_page(1).unwrap());
    let last = packet(&view);
    assert_eq!(view.score().unwrap().grades.as_ptr(), grade_pointer);
    assert_ne!(first, last);
    assert_eq!(
        first
            .iter()
            .filter(|row| row.0[1] < 514.)
            .collect::<Vec<_>>(),
        last.iter()
            .filter(|row| row.0[1] < 514.)
            .collect::<Vec<_>>()
    );
    assert!(!view.set_grade_page(1).unwrap());
    assert_eq!(packet(&view), last);
    assert!(view.set_grade_page(0).unwrap());
    assert_eq!(packet(&view), first);
}
#[test]
fn invalid_page_requests_refuse_without_mutating_page_score_or_composed_packet() {
    let mut view = HistoricalRecordPresentation::from_record(result(), Some(&score(5))).unwrap();
    view.set_grade_page(1).unwrap();
    let before = packet(&view);
    let pointer = view.score().unwrap().grades.as_ptr();
    for page in [2, usize::MAX] {
        assert!(view.set_grade_page(page).is_err());
        assert_eq!(view.grade_page(), 1);
        assert_eq!(view.score().unwrap().grades.as_ptr(), pointer);
        assert_eq!(packet(&view), before);
    }
}
