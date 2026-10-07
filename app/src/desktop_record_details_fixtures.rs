//! Deferred actual desktop Records subview routing with injected metadata only.
use super::*;
use beatkernel_bms_runtime::{
    competition::ScoreSummary,
    gauge::BmsGauge,
    local_players::PlayerId,
    play_result::{PlayResultScope, PlayResultOutcome},
    result_archive::{ArchivedResult, ArchivedScore},
};
fn historical(path: PathBuf) -> RecordPreview {
    let result = ArchivedResult {
        scope: PlayResultScope::FullSong,
        outcome: PlayResultOutcome::BelowClearThreshold,
        gauge: *BmsGauge::default().snapshot(),
    };
    let score = ScoreSummary {
        hits: 2,
        combo: 2,
        max_combo: 2,
        grades: [(u32::MAX, 2)].into_iter().collect(),
        ..Default::default()
    };
    RecordPreview {
        path,
        records: 0,
        recorded_until: None,
        start: beatkernel::time::Timestamp::ZERO,
        end: None,
        historical: Some((PlayerId(u32::MAX), result)),
        historical_score: Some(Arc::new(ArchivedScore::from_summary(&score).unwrap())),
        bms_score: None,
        historical_bms_score: None,
        historical_comparison: None,
        archive_error: None,
        score: Default::default(),
    }
}
fn prepared() -> Desktop {
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
    records.preview = Some(historical(PathBuf::from("record11.bkr")));
    app.sync_ime();
    app
}
#[test]
fn detail_button_or_nonrepeat_d_opens_under_same_parent_and_first_back_only_closes_subview() {
    for back in 0..3 {
        let mut app = prepared();
        let parent = app.navigator.back_target().unwrap();
        let child = app.navigator.active_id();
        if back == 0 {
            app.records.as_mut().unwrap().directory_focused = true;
            app.sync_ime();
            app.ime_event(Ime::Enabled);
            app.ime_event(Ime::Preedit("音".into(), None));
            assert!(app.ime.preview.is_some());
            app.activate(ControlId(66));
            assert!(app.ime.preview.is_none());
        } else {
            app.records_key(KeyCode::KeyD, false);
        }
        assert!(app.records.as_ref().unwrap().details);
        assert!(!app.records.as_ref().unwrap().directory_focused);
        assert!(app.text_target().is_none());
        assert_eq!(app.navigator.active_id(), child);
        app.draw().unwrap();
        assert_eq!(
            app.hits.iter().map(|hit| hit.0).collect::<Vec<_>>(),
            [ControlId(66)]
        );
        match back {
            0 => app.activate(ControlId(66)),
            1 => app.records_key(KeyCode::Escape, false),
            _ => app.back(),
        }
        let records = app.records.as_ref().unwrap();
        assert!(!records.details);
        assert_eq!(records.selected, Some(11));
        assert_eq!(records.first, 10);
        assert_eq!(app.navigator.active_id(), child);
        assert!(records.valid_preview().is_some());
        app.back();
        assert_eq!(app.navigator.route(), parent);
        assert!(app.records.is_none());
    }
}
#[test]
fn open_detail_mode_ignores_catalog_watch_opponent_paging_text_and_ime_actions() {
    let mut app = prepared();
    app.activate(ControlId(66));
    app.draw().unwrap();
    let directory = app.records.as_ref().unwrap().directory.clone();
    let selected = app.records.as_ref().unwrap().selected;
    let score = app
        .records
        .as_ref()
        .unwrap()
        .preview
        .as_ref()
        .unwrap()
        .historical_score
        .as_ref()
        .unwrap()
        .clone();
    let opponents = saved_opponents(&app.settings.as_ref().unwrap().values);
    for id in [50, 51, 52, 53, 54, 56, 57, 58, 59, 60, 61, 50012] {
        app.activate(ControlId(id));
    }
    for key in [
        KeyCode::KeyW,
        KeyCode::Enter,
        KeyCode::Tab,
        KeyCode::PageDown,
        KeyCode::ArrowDown,
        KeyCode::Backspace,
    ] {
        app.records_key(key, false);
    }
    app.keyboard_input(
        PhysicalKey::Code(KeyCode::KeyX),
        &Key::Character("new".into()),
        Some("new"),
        false,
    );
    app.ime_event(Ime::Preedit("音".into(), None));
    app.ime_event(Ime::Commit("音".into()));
    let records = app.records.as_ref().unwrap();
    assert!(records.details);
    assert_eq!(records.directory, directory);
    assert_eq!(records.selected, selected);
    assert_eq!(records.first, 10);
    assert!(Arc::ptr_eq(
        records
            .preview
            .as_ref()
            .unwrap()
            .historical_score
            .as_ref()
            .unwrap(),
        &score
    ));
    assert_eq!(
        saved_opponents(&app.settings.as_ref().unwrap().values),
        opponents
    );
    assert!(app.profile_io.is_none());
    assert!(app.game.is_none());
    assert!(app.text_target().is_none());
}
#[test]
fn repeated_d_directory_focus_stale_missing_and_pending_metadata_cannot_open_details() {
    for refusal in 0..5 {
        let mut app = prepared();
        match refusal {
            0 => {
                app.records_key(KeyCode::KeyD, true);
            }
            1 => {
                app.records.as_mut().unwrap().directory_focused = true;
                app.records_key(KeyCode::KeyD, false);
            }
            2 => {
                app.records.as_mut().unwrap().preview.as_mut().unwrap().path =
                    PathBuf::from("foreign.bkr");
                app.activate(ControlId(66));
            }
            3 => {
                app.records
                    .as_mut()
                    .unwrap()
                    .preview
                    .as_mut()
                    .unwrap()
                    .historical = None;
                app.activate(ControlId(66));
            }
            _ => {
                let (owner, permit) = app.metadata_scope().unwrap();
                app.profile_io = Some(ProfileOperation {
                    owner,
                    permit,
                    worker: None,
                });
                app.activate(ControlId(66));
                app.records_key(KeyCode::KeyD, false);
            }
        }
        assert!(!app.records.as_ref().unwrap().details);
        assert_eq!(app.navigator.route(), ScreenRoute::Records);
        assert_eq!(app.records.as_ref().unwrap().selected, Some(11));
    }
}
#[test]
fn selection_and_directory_replacement_invalidate_metadata_and_parent_disposal_releases_shared_details()
 {
    let mut app = prepared();
    app.activate(ControlId(66));
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
    app.records.as_mut().unwrap().select(12);
    assert!(!app.records.as_ref().unwrap().details);
    assert!(app.records.as_ref().unwrap().preview.is_none());
    app.draw().unwrap();
    let records = app.records.as_mut().unwrap();
    records.directory_focused = true;
    records.edit(None, Some("replacement/"));
    assert!(records.catalog.is_none());
    assert!(records.preview.is_none());
    assert!(!records.details);
    app.request_close();
    assert!(app.records.is_none());
    assert!(app.records_view.is_none());
    assert!(weak.upgrade().is_none());
}
