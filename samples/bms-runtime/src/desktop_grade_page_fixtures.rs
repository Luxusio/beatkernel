//! Deferred actual desktop grade controls and parent catalog lifetime.
use super::*;
use beatkernel_bms_runtime::{
    gauge::BmsGauge,
    local_players::PlayerId,
    play_result::{PlayResultScope, PlayResultOutcome},
    result_archive::{ArchivedResult, ArchivedScore},
    timing::TimingRecord,
};
fn preview(path: PathBuf, count: Option<usize>) -> RecordPreview {
    RecordPreview {
        path,
        records: 0,
        recorded_until: None,
        start: beatkernel::time::Timestamp::ZERO,
        end: None,
        historical: Some((
            PlayerId(u32::MAX),
            ArchivedResult {
                scope: PlayResultScope::FullSong,
                outcome: PlayResultOutcome::BelowClearThreshold,
                gauge: *BmsGauge::default().snapshot(),
            },
        )),
        historical_score: count.map(|count| {
            Arc::new(ArchivedScore {
                hits: count as u64,
                misses: 0,
                combo: 0,
                max_combo: 0,
                grades: (0..count)
                    .map(|index| {
                        (
                            if index + 1 == count {
                                u32::MAX
                            } else {
                                index as u32
                            },
                            1,
                        )
                    })
                    .collect(),
                timing: TimingRecord::default(),
            })
        }),
        historical_comparison: None,
        archive_error: None,
        score: Default::default(),
    }
}
fn prepared(count: Option<usize>) -> Desktop {
    let mut app = super::tests::lifecycle_fixture();
    app.open_settings();
    app.open_records();
    let records = app.records.as_mut().unwrap();
    records.catalog = Some(RecordCatalog {
        entries: (0..25)
            .map(|index| PathBuf::from(format!("record{index}.bkr")))
            .collect(),
        truncated: false,
    });
    records.select(11);
    records.preview = Some(preview(PathBuf::from("record11.bkr"), count));
    app.records_key(KeyCode::KeyD, false);
    app
}
#[test]
fn grade_direction_keys_home_end_and_repeat_clamp_without_mutating_catalog_selection() {
    let mut app = prepared(Some(9));
    let child = app.navigator.active_id();
    assert_eq!(app.records.as_ref().unwrap().grade_page, 0);
    for (key, page) in [
        (KeyCode::ArrowLeft, 0),
        (KeyCode::ArrowDown, 1),
        (KeyCode::PageDown, 2),
        (KeyCode::ArrowRight, 2),
        (KeyCode::ArrowUp, 1),
        (KeyCode::PageUp, 0),
        (KeyCode::End, 2),
        (KeyCode::Home, 0),
    ] {
        app.records_key(key, false);
        let records = app.records.as_ref().unwrap();
        assert_eq!(records.grade_page, page);
        assert_eq!(records.selected, Some(11));
        assert_eq!(records.first, 10);
        assert!(records.details);
        assert_eq!(app.navigator.active_id(), child);
    }
    for _ in 0..4 {
        app.records_key(KeyCode::ArrowRight, true);
    }
    assert_eq!(app.records.as_ref().unwrap().grade_page, 2);
}
#[test]
fn next_previous_buttons_reset_on_detail_back_and_reentry_under_same_parent() {
    let mut app = prepared(Some(9));
    let child = app.navigator.active_id();
    app.activate(ControlId(68));
    assert_eq!(app.records.as_ref().unwrap().grade_page, 1);
    app.activate(ControlId(68));
    assert_eq!(app.records.as_ref().unwrap().grade_page, 2);
    app.activate(ControlId(67));
    assert_eq!(app.records.as_ref().unwrap().grade_page, 1);
    app.activate(ControlId(66));
    assert!(!app.records.as_ref().unwrap().details);
    assert_eq!(app.records.as_ref().unwrap().grade_page, 0);
    assert_eq!(app.navigator.active_id(), child);
    assert_eq!(app.records.as_ref().unwrap().selected, Some(11));
    app.records_key(KeyCode::KeyD, false);
    assert_eq!(app.records.as_ref().unwrap().grade_page, 0);
    app.records_key(KeyCode::End, false);
    app.records_key(KeyCode::Escape, false);
    assert!(!app.records.as_ref().unwrap().details);
    assert_eq!(app.records.as_ref().unwrap().grade_page, 0);
    app.records.as_mut().unwrap().select(12);
    assert_eq!(app.records.as_ref().unwrap().grade_page, 0);
    assert!(app.records.as_ref().unwrap().preview.is_none());
    app.records.as_mut().unwrap().directory_focused = true;
    app.records
        .as_mut()
        .unwrap()
        .edit(None, Some("replacement/"));
    assert_eq!(app.records.as_ref().unwrap().grade_page, 0);
    assert!(app.records.as_ref().unwrap().catalog.is_none());
}
#[test]
fn unavailable_empty_one_page_and_pending_or_stale_metadata_cannot_advance_pages() {
    for count in [None, Some(0), Some(4)] {
        let mut app = prepared(count);
        app.records_key(KeyCode::End, false);
        app.activate(ControlId(68));
        assert_eq!(app.records.as_ref().unwrap().grade_page, 0);
        app.draw().unwrap();
        assert_eq!(
            app.hits.iter().map(|row| row.0).collect::<Vec<_>>(),
            [ControlId(66)]
        );
    }
    let mut app = prepared(Some(9));
    app.records_key(KeyCode::ArrowRight, false);
    let (owner, permit) = app.metadata_scope().unwrap();
    app.profile_io = Some(ProfileOperation {
        owner,
        permit,
        worker: None,
    });
    app.records_key(KeyCode::End, false);
    app.activate(ControlId(68));
    assert_eq!(app.records.as_ref().unwrap().grade_page, 1);
    app.profile_io = None;
    app.records.as_mut().unwrap().preview.as_mut().unwrap().path = PathBuf::from("foreign.bkr");
    app.records_key(KeyCode::End, false);
    app.activate(ControlId(68));
    assert_eq!(app.records.as_ref().unwrap().grade_page, 1);
}
#[test]
fn catalog_controls_never_route_grade_buttons_and_closing_releases_grade_metadata() {
    let mut app = prepared(Some(9));
    app.draw().unwrap();
    let weak = Arc::downgrade(
        app.records
            .as_ref()
            .unwrap()
            .preview
            .as_ref()
            .unwrap()
            .historical_score
            .as_ref()
            .unwrap(),
    );
    app.back();
    app.activate(ControlId(68));
    app.activate(ControlId(67));
    assert!(!app.records.as_ref().unwrap().details);
    assert_eq!(app.records.as_ref().unwrap().grade_page, 0);
    assert_eq!(app.records.as_ref().unwrap().selected, Some(11));
    app.records_key(KeyCode::KeyD, false);
    app.records_key(KeyCode::End, false);
    app.request_close();
    assert!(app.records.is_none());
    assert!(app.records_view.is_none());
    assert!(weak.upgrade().is_none());
}
