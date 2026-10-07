//! Deferred immutable Results composition; no backend or gameplay effects.
use super::*;
use crate::{
    gauge::BmsGauge,
    play_result::{CompletedPlayResult, PlayResultOutcome, PlayResultScope},
};
use beatkernel::{
    judge::{HazardEvent, HazardId, HazardOutcome},
    input::GameControlId,
    time::Timestamp,
};

fn result(start: i64, end: Option<i64>, fatal: bool) -> CompletedPlayResult {
    let mut gauge = BmsGauge::default();
    if fatal {
        gauge
            .observe(
                &[],
                &[HazardEvent {
                    id: HazardId(1),
                    at: Timestamp::ZERO,
                    control: GameControlId(0x11),
                    value: 1295,
                    outcome: HazardOutcome::Triggered,
                    input: None,
                }],
            )
            .unwrap();
    }
    CompletedPlayResult::from_completed(
        Timestamp::from_nanos(start),
        end.map(Timestamp::from_nanos),
        &gauge,
    )
}

#[test]
fn explicit_full_practice_and_failed_payloads_keep_original_identity_gauge_and_scope() {
    let full = result(0, None, false);
    let failed = result(0, None, true);
    let view = ResultsView::new(
        &[(PlayerId(u32::MAX), failed), (PlayerId(7), full)],
        &[PlayerId(7), PlayerId(u32::MAX)],
    )
    .unwrap();
    assert_eq!(
        view.rows().iter().map(|row| row.player).collect::<Vec<_>>(),
        [PlayerId(7), PlayerId(u32::MAX)]
    );
    assert_eq!(view.rows()[0].result, full);
    assert_eq!(view.rows()[1].result, failed);
    assert_eq!(view.rows()[0].result.gauge().level_units, 20_000_000);
    assert_eq!(view.rows()[1].result.gauge().level_units, 0);
    assert_eq!(
        view.rows()[0].result.outcome(),
        PlayResultOutcome::BelowClearThreshold
    );
    assert_eq!(
        view.rows()[1].result.outcome(),
        PlayResultOutcome::Failed(crate::gauge::GaugeFailure::InstantDeath)
    );
    assert_eq!(view.rows()[1].identity_label, "PLAYER 4294967295");
    assert_eq!(view.rows()[0].gauge_label, "GAUGE 20.000000%");
    assert_eq!(view.rows()[1].gauge_label, "GAUGE 0.000000%");
    assert_eq!(view.rows()[1].outcome_label, "FAILED - INSTANT DEATH");
    assert_eq!(view.scope_label(), "WHOLE SONG");
    assert!(!view.rows()[0].result.whole_song_clear());
    for (start, end) in [(0, Some(604_800_000_000_001)), (72_000_000_000_000, None)] {
        let practice = result(start, end, false);
        let view = ResultsView::new(&[(PlayerId(91), practice)], &[PlayerId(91)]).unwrap();
        assert_eq!(
            view.rows()[0].result.scope(),
            PlayResultScope::PracticeSection {
                start: Timestamp::from_nanos(start),
                end: end.map(Timestamp::from_nanos),
            }
        );
        assert!(!view.rows()[0].result.whole_song_clear());
        assert!(view.scope_label().contains("PRACTICE"));
    }
    let gauge = BmsGauge::new(
        crate::gauge::GaugeProfile::new(80_123_456, 80_000_000, 0, 0, false, vec![]).unwrap(),
    );
    for end in [None, Some(Timestamp::from_nanos(10))] {
        let clear = CompletedPlayResult::from_completed(Timestamp::ZERO, end, &gauge);
        let view = ResultsView::new(&[(PlayerId(9), clear)], &[PlayerId(9)]).unwrap();
        assert_eq!(view.rows()[0].gauge_label, "GAUGE 80.123456%");
        assert_eq!(
            view.rows()[0].outcome_label,
            if end.is_some() {
                "PRACTICE - CLEAR THRESHOLD MET"
            } else {
                "CLEARED"
            }
        );
        assert_eq!(clear.whole_song_clear(), end.is_none());
    }
}

#[test]
fn malformed_later_rows_rosters_and_mixed_scopes_refuse_the_entire_view() {
    let full = result(0, None, false);
    let valid = [(PlayerId(7), full), (PlayerId(u32::MAX), full)];
    let roster = [PlayerId(7), PlayerId(u32::MAX)];
    for rows in [
        vec![],
        vec![valid[0]],
        vec![valid[0], valid[0]],
        vec![valid[0], (PlayerId(0), full)],
        vec![valid[0], (PlayerId(9), full)],
        vec![valid[0], (valid[1].0, result(1, None, false))],
        vec![valid[0], (valid[1].0, result(0, Some(10), false))],
    ] {
        assert!(ResultsView::new(&rows, &roster).is_err());
    }
    for invalid_roster in [
        vec![],
        vec![PlayerId(0), PlayerId(u32::MAX)],
        vec![PlayerId(7), PlayerId(7)],
        vec![PlayerId(7), PlayerId(9)],
    ] {
        assert!(ResultsView::new(&valid, &invalid_roster).is_err());
    }
    let roster = (1..=65).map(PlayerId).collect::<Vec<_>>();
    let rows = roster.iter().map(|&id| (id, full)).collect::<Vec<_>>();
    assert!(ResultsView::new(&rows, &roster).is_err());
    // Failed construction cannot replace another owner's immutable accepted view.
    let accepted = ResultsView::new(&valid, &[PlayerId(7), PlayerId(u32::MAX)]).unwrap();
    assert_eq!(
        accepted
            .rows()
            .iter()
            .map(|row| (row.player, row.result))
            .collect::<Vec<_>>(),
        valid
    );
}

#[test]
fn sixty_four_nonconsecutive_players_have_four_per_page_and_repeatable_retained_composition() {
    let full = result(0, None, false);
    let roster = (0..64)
        .map(|index| PlayerId(u32::MAX - index * 17))
        .collect::<Vec<_>>();
    let rows = roster
        .iter()
        .rev()
        .map(|&id| (id, full))
        .collect::<Vec<_>>();
    let view = ResultsView::new(&rows, &roster).unwrap();
    assert_eq!(view.page_count(), 16);
    assert_eq!(
        view.rows().iter().map(|row| row.player).collect::<Vec<_>>(),
        roster
    );
    let formatted_ptrs = view
        .rows()
        .iter()
        .map(|row| {
            (
                row.identity_label.as_ptr(),
                row.outcome_label.as_ptr(),
                row.gauge_label.as_ptr(),
            )
        })
        .collect::<Vec<_>>();
    let packets_ptr = view.pages.as_ptr();
    let mut scene = crate::scene::Scene::new(960, 720);
    for page in [0, 15, 0, 15] {
        scene.clear();
        view.compose(&mut scene, page).unwrap();
        scene.status().unwrap();
        assert_eq!(
            view.rows()
                .iter()
                .map(|row| (
                    row.identity_label.as_ptr(),
                    row.outcome_label.as_ptr(),
                    row.gauge_label.as_ptr(),
                ))
                .collect::<Vec<_>>(),
            formatted_ptrs
        );
        assert_eq!(view.pages.as_ptr(), packets_ptr);
        assert!(scene.playfields().is_empty());
        let row_positions = scene
            .rectangles()
            .iter()
            .map(|rectangle| rectangle.bounds[1] as i64)
            .filter(|&y| [140, 250, 360, 470].contains(&y))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(row_positions, [140, 250, 360, 470].into_iter().collect());
    }
    let old_stamp = scene.geometry_stamp().1;
    assert!(view.compose(&mut scene, 16).is_err());
    assert_eq!(scene.geometry_stamp().1, old_stamp);
    assert!(view.compose(&mut scene, usize::MAX).is_err());
    assert_eq!(scene.geometry_stamp().1, old_stamp);
}
