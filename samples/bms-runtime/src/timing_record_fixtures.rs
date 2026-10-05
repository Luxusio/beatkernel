//! Deferred independent historical timing shape and exact-integer fixtures.
use super::*;
use beatkernel::{
    chart::ObjectId,
    judge::{JudgeEvent, JudgeGrade, JudgeOutcome, JudgeStage},
    time::{Duration, Timestamp},
};

fn event(delta: i64, stage: JudgeStage) -> JudgeEvent {
    JudgeEvent {
        object: ObjectId(1),
        stage,
        outcome: JudgeOutcome::Hit {
            grade: JudgeGrade(u32::MAX),
            delta: Duration::from_nanos(delta),
        },
        at: Timestamp::ZERO,
        input: None,
    }
}
fn empty() -> TimingRecord {
    TimingRecord {
        count: 0,
        early: 0,
        late: 0,
        exact: 0,
        sum: 0,
        absolute_sum: 0,
        last: None,
        min: None,
        max: None,
    }
}
#[test]
fn actual_known_stage_samples_export_exact_extremes_and_ignore_custom_stage() {
    let mut summary = TimingSummary::default();
    summary
        .observe(&[
            event(i64::MIN, JudgeStage::HoldHead),
            event(i64::MAX, JudgeStage::HoldTail),
            event(0, JudgeStage::Instant),
            event(99, JudgeStage::Custom(7)),
        ])
        .unwrap();
    assert_eq!(
        summary.record(),
        TimingRecord {
            count: 3,
            early: 1,
            late: 1,
            exact: 1,
            sum: -1,
            absolute_sum: u64::MAX as u128,
            last: Some(0),
            min: Some(i64::MIN),
            max: Some(i64::MAX)
        }
    );
    summary.record().validate().unwrap();
    assert_eq!(TimingSummary::default().record(), empty());
}
#[test]
fn empty_record_requires_zero_scalars_and_absent_extrema() {
    empty().validate().unwrap();
    for field in 0..9 {
        let mut record = empty();
        match field {
            0 => record.early = 1,
            1 => record.late = 1,
            2 => record.exact = 1,
            3 => record.sum = 1,
            4 => record.absolute_sum = 1,
            5 => record.last = Some(0),
            6 => record.min = Some(0),
            7 => record.max = Some(0),
            _ => {
                record.count = 1;
                record.exact = 1;
            }
        }
        assert!(
            record.validate().is_err(),
            "invalid empty/option field {field}"
        );
    }
}
#[test]
fn bucket_addition_and_128_bit_magnitude_bounds_preserve_full_width() {
    // Public historical numeric boundary injection, not a runtime sample claim.
    let count = u64::MAX;
    let positive = (count as i128) * (i64::MAX as i128);
    let record = TimingRecord {
        count,
        early: 0,
        late: count,
        exact: 0,
        sum: positive,
        absolute_sum: positive as u128,
        last: Some(i64::MAX),
        min: Some(i64::MAX),
        max: Some(i64::MAX),
    };
    record.validate().unwrap();
    let negative = (count as i128) * (i64::MIN as i128);
    let record = TimingRecord {
        count,
        early: count,
        late: 0,
        exact: 0,
        sum: negative,
        absolute_sum: negative.unsigned_abs(),
        last: Some(i64::MIN),
        min: Some(i64::MIN),
        max: Some(i64::MIN),
    };
    record.validate().unwrap();
    let mut overflow = record;
    overflow.exact = 1;
    assert!(overflow.validate().is_err());
    let mut excessive = record;
    excessive.absolute_sum = u128::MAX;
    assert!(excessive.validate().is_err());
    let mut excessive = record;
    excessive.sum = i128::MAX;
    assert!(excessive.validate().is_err());
}
#[test]
fn nonempty_options_extrema_last_and_signs_must_be_consistent() {
    let valid = TimingRecord {
        count: 2,
        early: 1,
        late: 1,
        exact: 0,
        sum: 1,
        absolute_sum: 5,
        last: Some(3),
        min: Some(-2),
        max: Some(3),
    };
    valid.validate().unwrap();
    for field in 0..9 {
        let mut bad = valid;
        match field {
            0 => bad.last = None,
            1 => bad.min = None,
            2 => bad.max = None,
            3 => bad.last = Some(4),
            4 => bad.min = Some(4),
            5 => bad.max = Some(-3),
            6 => bad.early = 0,
            7 => bad.late = 0,
            _ => bad.count = 3,
        }
        assert!(bad.validate().is_err(), "invalid timing field {field}");
    }
    let mut wrong_sign = valid;
    wrong_sign.early = 0;
    wrong_sign.late = 2;
    assert!(wrong_sign.validate().is_err());
}
#[test]
fn achievable_magnitude_and_zero_bucket_shape_reject_impossible_history() {
    let zero = TimingRecord {
        count: 9_007_199_254_740_993,
        exact: 9_007_199_254_740_993,
        last: Some(0),
        min: Some(0),
        max: Some(0),
        ..empty()
    };
    zero.validate().unwrap();
    let mut bad = zero;
    bad.sum = 1;
    assert!(bad.validate().is_err());
    let mut bad = zero;
    bad.absolute_sum = 1;
    assert!(bad.validate().is_err());
    let one = TimingRecord {
        count: 1,
        early: 0,
        late: 1,
        exact: 0,
        sum: 3,
        absolute_sum: 3,
        last: Some(3),
        min: Some(3),
        max: Some(3),
    };
    one.validate().unwrap();
    for (sum, absolute_sum) in [(0, 0), (4, 4), (-3, 3), (3, 2)] {
        assert!(
            TimingRecord {
                sum,
                absolute_sum,
                ..one
            }
            .validate()
            .is_err()
        );
    }
}
