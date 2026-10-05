//! Deferred atomic result-table presentation; private fixture access only.
use super::*;
use crate::play_result::{CompletedPlayResult, PlayResultScope};

fn result(gauge: &BmsGauge, end: Option<i64>) -> CompletedPlayResult {
    CompletedPlayResult::from_completed(Timestamp::ZERO, end.map(Timestamp::from_nanos), gauge)
}
fn snapshot() -> PlayerSnapshot {
    let mut value = PlayerSnapshot::default();
    value.players = vec![
        LocalPlayerSnapshot::new(PlayerId(7), None),
        LocalPlayerSnapshot::new(PlayerId(u32::MAX), None),
    ];
    value.players[0].pressed_lanes = 1;
    value.players[0].score.hits = 3;
    value.players[1].song_time = Some(Timestamp::from_nanos(19));
    value
}
fn rows(value: &PlayerSnapshot) -> Vec<(PlayerId, CompletedPlayResult)> {
    value
        .players
        .iter()
        .map(|member| (member.player, result(&member.gauge, None)))
        .collect()
}

#[test]
fn result_table_rejects_missing_unknown_duplicate_later_gauge_or_scope_without_partial_state() {
    let original = snapshot();
    let valid = rows(&original);
    let changed_gauge = crate::native_solo_result_fixtures::cleared_gauge();
    let cases = vec![
        vec![],
        vec![valid[0]],
        vec![valid[0], (PlayerId(9), valid[1].1)],
        vec![valid[0], valid[0]],
        vec![valid[0], (PlayerId(0), valid[1].1)],
        vec![valid[0], (valid[1].0, result(&changed_gauge, None))],
        vec![
            valid[0],
            (valid[1].0, result(&original.players[1].gauge, Some(10))),
        ],
    ];
    for candidate in cases {
        let mut value = original.clone();
        assert!(value.apply_completed_results(&candidate).is_err());
        assert_eq!(value.completed_results, None);
        assert_eq!(value.players[0].pressed_lanes, 1);
        assert_eq!(value.players[0].score.hits, 3);
        assert_eq!(value.players[1].song_time, Some(Timestamp::from_nanos(19)));
        assert_eq!(value.players[0].gauge, original.players[0].gauge);
        assert_eq!(value.players[1].gauge, original.players[1].gauge);
    }
    for count in [0, 65] {
        let mut value = PlayerSnapshot::default();
        value.players = (1..=count)
            .map(|id| LocalPlayerSnapshot::new(PlayerId(id), None))
            .collect();
        let candidate = rows(&value);
        assert!(value.apply_completed_results(&candidate).is_err());
        assert_eq!(value.completed_results, None);
    }
    let mut duplicate_roster = original.clone();
    duplicate_roster.players[1].player = PlayerId(7);
    assert!(duplicate_roster.apply_completed_results(&valid).is_err());
    assert_eq!(duplicate_roster.completed_results, None);
}

#[test]
fn first_table_is_canonical_idempotent_and_immutable_with_history_untouched() {
    let mut value = snapshot();
    let valid = rows(&value);
    let reversed = vec![valid[1], valid[0]];
    assert!(value.apply_completed_results(&reversed).unwrap());
    assert_eq!(value.completed_results, Some(valid.clone()));
    assert!(!value.apply_completed_results(&valid).unwrap());
    assert!(!value.apply_completed_results(&reversed).unwrap());
    let changed = value
        .players
        .iter()
        .map(|member| (member.player, result(&member.gauge, Some(10))))
        .collect::<Vec<_>>();
    assert!(value.apply_completed_results(&changed).is_err());
    assert_eq!(value.completed_results, Some(valid));
    assert_eq!(value.players[0].score.hits, 3);
    assert_eq!(value.players[0].pressed_lanes, 1);
    assert_eq!(value.players[1].song_time, Some(Timestamp::from_nanos(19)));
    assert_eq!(value.status, PlayerStatus::Loading);
    assert_eq!(value.completed_end, None);
}

#[test]
fn first_completion_is_visible_before_cleanup_and_survives_cleanup_failure_without_fabricating_finished_result()
 {
    let source =
        beatkernel_bms::parse("#BPM 120\n#WAV01 tap.wav\n#00011:01\n", Default::default()).unwrap();
    let chart = source.compile().unwrap().chart;
    let gauge = BmsGauge::default();
    let completed = result(&gauge, Some(10));
    let table = vec![(PlayerId(7), completed), (PlayerId(u32::MAX), completed)];
    let (publisher, viewer) = channel();
    assert_eq!(viewer.take_latest().unwrap().completed_results, None);
    let outcome = with_publisher::<()>(publisher, || {
        publish_local_chart(&source, &chart, &[PlayerId(7), PlayerId(u32::MAX)]).unwrap();
        viewer.take_latest();
        publish_completed_local(&table).unwrap();
        let before_cleanup = viewer
            .take_latest()
            .expect("first completion bypasses report cadence");
        assert_eq!(before_cleanup.completed_results, Some(table.clone()));
        assert_eq!(before_cleanup.status, PlayerStatus::Loading);
        assert_eq!(
            before_cleanup.completed_results.as_ref().unwrap()[0]
                .1
                .scope(),
            PlayResultScope::PracticeSection {
                start: Timestamp::ZERO,
                end: Some(Timestamp::from_nanos(10))
            }
        );
        publish_completed_local(&table).unwrap();
        assert!(
            viewer.take_latest().is_none(),
            "identical completion does not republish"
        );
        assert!(
            publish_completed_solo(completed).is_err(),
            "solo cannot select one member from a cohort"
        );
        Err("cleanup join failed".into())
    });
    assert_eq!(outcome, Err("cleanup join failed".into()));
    let historical = viewer.take_latest().unwrap();
    assert_eq!(historical.completed_results, Some(table));
    assert_eq!(
        historical.status,
        PlayerStatus::Failed("cleanup join failed".into())
    );
    let (publisher, viewer) = channel();
    with_publisher(publisher, || {
        publish_local_chart(&source, &chart, &[PlayerId(u32::MAX)]).unwrap();
        viewer.take_latest();
        publish_completed_solo(completed).unwrap();
        assert_eq!(
            viewer.take_latest().unwrap().completed_results,
            Some(vec![(PlayerId(u32::MAX), completed)])
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(
        viewer.take_latest().unwrap().completed_results,
        Some(vec![(PlayerId(u32::MAX), completed)])
    );
    let (publisher, viewer) = channel();
    with_publisher(publisher, || Ok(())).unwrap();
    let cancelled_or_empty = viewer.take_latest().unwrap();
    assert_eq!(cancelled_or_empty.status, PlayerStatus::Finished);
    assert_eq!(cancelled_or_empty.completed_results, None);
    publish_completed_solo(completed).unwrap(); // Unattached is an explicit no-op.
    publish_completed_local(&[]).unwrap();
}
