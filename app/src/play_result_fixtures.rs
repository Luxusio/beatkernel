//! Deferred classification only: the internal constructor assumes proven completion.
use crate::{
    gauge::{BmsGauge, GaugeFailure, GaugeProfile, GaugeSnapshot},
    play_result::{CompletedPlayResult, PlayResultOutcome, PlayResultScope},
};
use beatkernel::{
    input::GameControlId,
    judge::{HazardEvent, HazardId, HazardOutcome},
    time::Timestamp,
};

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn gauge(level: u64) -> BmsGauge {
    BmsGauge::new(
        GaugeProfile::new(level, 80_000_000, 1_000_000, -6_000_000, false, vec![]).unwrap(),
    )
}

#[test]
fn completed_threshold_is_exact_and_retains_the_original_gauge_without_observing_again() {
    for (level, outcome) in [
        (0, PlayResultOutcome::BelowClearThreshold),
        (79_999_999, PlayResultOutcome::BelowClearThreshold),
        (80_000_000, PlayResultOutcome::Cleared),
        (80_000_001, PlayResultOutcome::Cleared),
        (100_000_000, PlayResultOutcome::Cleared),
    ] {
        let value = gauge(level);
        let before = value.clone();
        let result = CompletedPlayResult::from_completed(ts(0), None, &value);
        assert_eq!(result.scope(), PlayResultScope::FullSong);
        assert_eq!(result.outcome(), outcome);
        assert_eq!(
            result.gauge(),
            GaugeSnapshot {
                level_units: level,
                failure: None
            }
        );
        assert_eq!(result.whole_song_clear(), level >= 80_000_000);
        assert_eq!(value, before);
        assert_eq!(
            CompletedPlayResult::from_completed(ts(0), None, &value),
            result
        );
    }
    assert_eq!(
        CompletedPlayResult::from_completed(ts(0), None, &BmsGauge::default()).outcome(),
        PlayResultOutcome::BelowClearThreshold
    );
}

#[test]
fn practice_scope_and_first_failure_prevent_whole_song_clear_even_at_a_zero_threshold() {
    let full = gauge(100_000_000);
    for (start, end) in [
        (0, Some(4_000_000_000)),
        (2_000_000_000, None),
        (2_000_000_000, Some(4_000_000_000)),
    ] {
        let result = CompletedPlayResult::from_completed(ts(start), end.map(ts), &full);
        assert_eq!(
            result.scope(),
            PlayResultScope::PracticeSection {
                start: ts(start),
                end: end.map(ts)
            }
        );
        assert_eq!(result.outcome(), PlayResultOutcome::Cleared);
        assert_eq!(result.gauge(), *full.snapshot());
        assert!(!result.whole_song_clear());
    }
    let depleted = BmsGauge::new(GaugeProfile::new(0, 0, 0, 0, true, vec![]).unwrap());
    let mut fatal = BmsGauge::new(GaugeProfile::new(100_000_000, 0, 0, 0, false, vec![]).unwrap());
    let historical = CompletedPlayResult::from_completed(ts(0), None, &fatal);
    fatal
        .observe(
            &[],
            &[HazardEvent {
                id: HazardId(u64::MAX),
                at: ts(7),
                control: GameControlId(0x11),
                value: 1295,
                outcome: HazardOutcome::Triggered,
                input: None,
            }],
        )
        .unwrap();
    for (value, reason) in [
        (&depleted, GaugeFailure::Depleted),
        (&fatal, GaugeFailure::InstantDeath),
    ] {
        let result = CompletedPlayResult::from_completed(ts(0), None, value);
        assert_eq!(result.outcome(), PlayResultOutcome::Failed(reason));
        assert_eq!(
            result.gauge(),
            GaugeSnapshot {
                level_units: 0,
                failure: Some(reason)
            }
        );
        assert!(!result.whole_song_clear());
    }
    assert_eq!(
        historical.gauge(),
        GaugeSnapshot {
            level_units: 100_000_000,
            failure: None
        }
    );
    assert!(
        historical.whole_song_clear(),
        "a result owns its historical snapshot rather than borrowing mutable gauge state"
    );
}
