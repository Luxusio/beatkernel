//! Deferred actual Records controller navigation over common historical page counts.
use super::*;
use beatkernel_bms_runtime::{
    gauge::BmsGauge,
    local_players::PlayerId,
    play_result::{PlayResultScope, PlayResultOutcome},
    result_archive::{ArchivedResult, ArchivedScore},
    timing::TimingRecord,
    competition::OpponentKind,
    competition_presentation::{
        CompetitionSnapshot, GhostSnapshot, NetworkSnapshot, NetworkStatus,
    },
};
fn comparison(ghosts: usize) -> CompetitionSnapshot {
    CompetitionSnapshot {
        ghosts: (0..ghosts)
            .map(|index| GhostSnapshot {
                kind: OpponentKind::Own,
                label: format!("saved{index}.bkr"),
                hits: 4,
                misses: 1,
                combo: 2,
                max_combo: 4,
                recorded_until: None,
            })
            .collect(),
        network: Some(NetworkSnapshot {
            status: NetworkStatus::Stopped,
            progress: None,
        }),
    }
}
fn prepared(grades: usize, comparison: Option<Option<CompetitionSnapshot>>) -> Desktop {
    let mut app = super::tests::lifecycle_fixture();
    app.open_settings();
    app.open_records();
    let path = PathBuf::from("original.bkr");
    let records = app.records.as_mut().unwrap();
    records.catalog = Some(RecordCatalog {
        entries: vec![path.clone(), PathBuf::from("replacement.bkr")],
        truncated: false,
    });
    records.select(0);
    records.preview = Some(RecordPreview {
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
        historical_score: Some(Arc::new(ArchivedScore {
            hits: grades as u64,
            misses: 0,
            combo: 0,
            max_combo: 0,
            grades: (0..grades).map(|grade| (grade as u32, 1)).collect(),
            timing: TimingRecord::default(),
        })),
        bms_score: None,
        historical_bms_score: None,
        historical_comparison: comparison.map(Arc::new),
        archive_error: None,
        score: Default::default(),
    });
    app.records_key(KeyCode::KeyD, false);
    app
}
#[test]
fn end_and_direction_keys_use_total_1033_pages_and_keep_legacy_empty_availability_counts() {
    for (metadata, last) in [
        (None, 1023),
        (Some(None), 1024),
        (
            Some(Some(CompetitionSnapshot {
                ghosts: vec![],
                network: None,
            })),
            1024,
        ),
        (Some(Some(comparison(8))), 1032),
    ] {
        let mut app = prepared(4096, metadata);
        let child = app.navigator.active_id();
        app.records_key(KeyCode::End, false);
        assert_eq!(app.records.as_ref().unwrap().grade_page, last);
        app.records_key(KeyCode::ArrowRight, false);
        assert_eq!(app.records.as_ref().unwrap().grade_page, last);
        app.records_key(KeyCode::ArrowLeft, false);
        assert_eq!(app.records.as_ref().unwrap().grade_page, last - 1);
        app.records_key(KeyCode::Home, false);
        assert_eq!(app.records.as_ref().unwrap().grade_page, 0);
        assert_eq!(app.records.as_ref().unwrap().selected, Some(0));
        assert_eq!(app.navigator.active_id(), child);
    }
}
#[test]
fn buttons_cross_grade_to_saved_to_peer_pages_and_back_reentry_resets_same_parent_frontier() {
    let mut app = prepared(5, Some(Some(comparison(1))));
    let child = app.navigator.active_id();
    app.activate(ControlId(68));
    app.activate(ControlId(68));
    assert_eq!(app.records.as_ref().unwrap().grade_page, 2);
    app.draw().unwrap();
    assert!(app.hits.iter().any(|row| row.0 == ControlId(68)));
    app.activate(ControlId(68));
    assert_eq!(app.records.as_ref().unwrap().grade_page, 3);
    app.draw().unwrap();
    assert!(!app.hits.iter().any(|row| row.0 == ControlId(68)));
    assert!(app.hits.iter().any(|row| row.0 == ControlId(67)));
    app.activate(ControlId(66));
    assert_eq!(app.records.as_ref().unwrap().grade_page, 0);
    assert!(!app.records.as_ref().unwrap().details);
    assert_eq!(app.navigator.active_id(), child);
    app.records_key(KeyCode::KeyD, false);
    assert_eq!(app.records.as_ref().unwrap().grade_page, 0);
}
#[test]
fn changed_record_resets_pages_pending_guard_blocks_navigation_and_parent_close_releases_comparison_arc()
 {
    let mut app = prepared(5, Some(Some(comparison(1))));
    app.records_key(KeyCode::End, false);
    app.draw().unwrap();
    let weak = Arc::downgrade(
        app.records
            .as_ref()
            .unwrap()
            .preview
            .as_ref()
            .unwrap()
            .historical_comparison
            .as_ref()
            .unwrap(),
    );
    let (owner, permit) = app.metadata_scope().unwrap();
    app.profile_io = Some(ProfileOperation {
        owner,
        permit,
        worker: None,
    });
    app.records_key(KeyCode::Home, false);
    assert_eq!(app.records.as_ref().unwrap().grade_page, 3);
    app.profile_io = None;
    app.back();
    app.records.as_mut().unwrap().select(1);
    assert_eq!(app.records.as_ref().unwrap().grade_page, 0);
    assert!(app.records.as_ref().unwrap().preview.is_none());
    app.draw().unwrap();
    app.request_close();
    assert!(app.records.is_none());
    assert!(app.records_view.is_none());
    assert!(weak.upgrade().is_none());
}
