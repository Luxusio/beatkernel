//! Deferred admission cadence scalar policy, distinct from transport receipts.
use super::*;
#[test]
fn first_admission_and_exact_fifty_millisecond_boundary_use_integer_observations() {
    let mut cadence = ProgressCadence::new();
    assert!(cadence.observe(0));
    assert!(cadence.due(false));
    cadence.admitted();
    assert_eq!(cadence.last_published(), Some(0));
    for (now, due) in [(49_999_999, false), (50_000_000, true), (50_000_001, true)] {
        assert!(cadence.observe(now));
        assert_eq!(cadence.due(false), due);
    }
    cadence.admitted();
    assert_eq!(cadence.last_published(), Some(50_000_001));
}
#[test]
fn observation_without_admission_never_consumes_marker_and_final_force_bypasses_only_interval() {
    let mut cadence = ProgressCadence::new();
    cadence.observe(0);
    cadence.admitted();
    assert!(cadence.observe(1));
    assert!(!cadence.due(false));
    assert!(cadence.due(true));
    assert_eq!(cadence.last_published(), Some(0));
    assert!(cadence.observe(50_000_000));
    assert!(cadence.due(false));
    // Caller refusal leaves admission uncommitted; the next valid attempt stays due.
    assert!(cadence.observe(50_000_001));
    assert!(cadence.due(false));
    assert_eq!(cadence.last_published(), Some(0));
    cadence.admitted();
    assert_eq!(cadence.last_published(), Some(50_000_001));
}
#[test]
fn regression_after_suppression_preserves_last_valid_observation_and_successful_marker() {
    let mut cadence = ProgressCadence::new();
    cadence.observe(0);
    cadence.admitted();
    assert!(cadence.observe(49_999_999));
    assert!(!cadence.due(false));
    assert!(!cadence.observe(49_999_998));
    assert_eq!(cadence.last_observed(), Some(49_999_999));
    assert_eq!(cadence.last_published(), Some(0));
    assert!(cadence.observe(50_000_000));
    assert!(cadence.due(false));
    cadence.admitted();
    assert_eq!(cadence.last_published(), Some(50_000_000));
}
#[test]
fn week_above_number_precision_and_unsigned_maximum_keep_exact_boundary_without_overflow() {
    for origin in [
        604_800_000_000_000u64,
        9_007_199_254_740_993,
        u64::MAX - 50_000_000,
    ] {
        let mut cadence = ProgressCadence::new();
        assert!(cadence.observe(origin));
        assert!(cadence.due(false));
        cadence.admitted();
        assert!(cadence.observe(origin + 49_999_999));
        assert!(!cadence.due(false));
        assert!(cadence.observe(origin + 50_000_000));
        assert!(cadence.due(false));
        cadence.admitted();
        assert_eq!(cadence.last_published(), Some(origin + 50_000_000));
    }
}
