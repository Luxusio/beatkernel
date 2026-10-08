//! Integer presentation of accepted judge timing; never owns clocks or calibration.
use beatkernel::{
    judge::{JudgeEvent, JudgeGrade, JudgeOutcome},
    time::Duration,
};

/// Unsigned milliseconds with three fractional digits, truncating sub-microseconds.
pub fn unsigned_ms(ns: u64) -> String {
    format!("{}.{:03} MS", ns / 1_000_000, (ns % 1_000_000) / 1000)
}
/// Signed bias preserves a negative sign even below the displayed resolution.
pub fn signed_ms(ns: i64) -> String {
    let sign = if ns < 0 {
        "-"
    } else if ns > 0 {
        "+"
    } else {
        ""
    };
    format!("{sign}{}", unsigned_ms(ns.unsigned_abs()))
}
/// Actual accepted hit label and its timing category color.
pub fn hit_label(grade: JudgeGrade, delta: Duration) -> (String, u32) {
    let ns = delta.as_nanos();
    let (category, color) = if ns < 0 {
        ("EARLY", 0x87bfff)
    } else if ns > 0 {
        ("LATE", 0xffc08e)
    } else {
        ("EXACT", 0x74e5c5)
    };
    if ns == 0 {
        (format!("G{} {category}", grade.0), color)
    } else {
        (
            format!("G{} {category} {}", grade.0, unsigned_ms(ns.unsigned_abs())),
            color,
        )
    }
}
/// Misses remain explicit without inventing an input error or zero timing sample.
pub fn judge_label(event: &JudgeEvent) -> (String, u32) {
    match event.outcome {
        JudgeOutcome::Hit { grade, delta } => hit_label(grade, delta),
        JudgeOutcome::Miss { .. } => ("MISS".into(), 0xff8e8e),
    }
}
/// Full accepted-stage bias and mean absolute error, with explicit empty state.
pub fn summary(timing: &crate::timing::TimingSummary) -> (String, String) {
    record(&timing.record())
}
/// Read-only timing labels shared by native and transferred menu values.
pub fn record(timing: &crate::timing::TimingRecord) -> (String, String) {
    let mean = (timing.count != 0)
        .then(|| i64::try_from(timing.sum / i128::from(timing.count)).ok())
        .flatten();
    let absolute = (timing.count != 0)
        .then(|| u64::try_from(timing.absolute_sum / u128::from(timing.count)).ok())
        .flatten();
    (
        format!("BIAS {}", mean.map_or("--".into(), signed_ms)),
        format!("MEAN ABS {}", absolute.map_or("--".into(), unsigned_ms)),
    )
}

#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn integer_labels_preserve_sign_and_signed_minimum_without_rounding_into_exact() {
        let grade = JudgeGrade(u32::MAX);
        assert_eq!(
            hit_label(grade, Duration::ZERO),
            ("G4294967295 EXACT".into(), 0x74e5c5)
        );
        assert_eq!(
            hit_label(JudgeGrade(7), Duration::from_nanos(-1)),
            ("G7 EARLY 0.000 MS".into(), 0x87bfff)
        );
        assert_eq!(
            hit_label(JudgeGrade(7), Duration::from_nanos(1)),
            ("G7 LATE 0.000 MS".into(), 0xffc08e)
        );
        assert_eq!(signed_ms(-1), "-0.000 MS");
        assert_eq!(signed_ms(1_234_999), "+1.234 MS");
        assert_eq!(signed_ms(i64::MIN), "-9223372036854.775 MS");
        assert_eq!(signed_ms(i64::MAX), "+9223372036854.775 MS");
        assert_eq!(unsigned_ms(u64::MAX), "18446744073709.551 MS");
        assert_eq!(
            summary(&crate::timing::TimingSummary::default()),
            ("BIAS --".into(), "MEAN ABS --".into())
        );
    }
}
