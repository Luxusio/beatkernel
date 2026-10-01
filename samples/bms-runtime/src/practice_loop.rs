//! Exact observed practice endpoints; native restart ownership stays external.
use crate::practice::PracticeStart;
use beatkernel::time::{Duration, Timestamp};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PracticeLoop {
    start: PracticeStart,
    end: PracticeStart,
}
impl PracticeLoop {
    pub fn new(start: PracticeStart, end: PracticeStart) -> Result<Self, String> {
        if end.nanoseconds() <= start.nanoseconds() {
            return Err("practice loop end must follow its start".into());
        }
        Ok(Self { start, end })
    }
    pub const fn start(self) -> PracticeStart {
        self.start
    }
    pub const fn end(self) -> PracticeStart {
        self.end
    }
    /// Uses the observed integer song position; equality reaches the endpoint.
    pub fn reached(self, song: Timestamp) -> bool {
        song.as_nanos() >= self.end.nanoseconds()
    }
    /// Exclusive end on a fresh session's playback grid, rounded up once.
    /// Inserted physical pause frames never enter this mapping.
    pub fn playback_end_frame(
        self,
        session_start: PracticeStart,
        preroll: Duration,
        sample_rate: u32,
    ) -> Result<u64, String> {
        if session_start != self.start || preroll.as_nanos() < 0 || sample_rate == 0 {
            return Err(
                "loop endpoint requires matching start, nonnegative preroll and nonzero rate"
                    .into(),
            );
        }
        let nanos = i128::from(self.end.nanoseconds()) - i128::from(session_start.nanoseconds())
            + i128::from(preroll.as_nanos());
        let frames = nanos
            .checked_mul(i128::from(sample_rate))
            .and_then(|value| value.checked_add(999_999_999))
            .map(|value| value / 1_000_000_000)
            .and_then(|value| u64::try_from(value).ok())
            .ok_or("loop playback endpoint exceeds u64 frames")?;
        Ok(frames)
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    fn start(ns: i64) -> PracticeStart {
        PracticeStart::from_nanoseconds(ns).unwrap()
    }
    #[test]
    fn one_nanosecond_region_has_exact_inclusive_end_and_negative_positions_never_reach() {
        let region = PracticeLoop::new(start(0), start(1)).unwrap();
        assert_eq!(region.start(), start(0));
        assert_eq!(region.end(), start(1));
        for ns in [i64::MIN, -1, 0] {
            assert!(!region.reached(Timestamp::from_nanos(ns)));
        }
        assert!(region.reached(Timestamp::from_nanos(1)));
        assert!(region.reached(Timestamp::from_nanos(2)));
        let preserved = region;
        assert!(PracticeLoop::new(start(1), start(1)).is_err());
        assert!(PracticeLoop::new(start(2), start(1)).is_err());
        assert_eq!(region, preserved);
    }
    #[test]
    fn long_endpoints_require_no_duration_arithmetic_or_overflow() {
        for (first, last) in [
            (72_000_000_000_000, 72_000_000_000_001),
            (604_800_000_000_000, 604_800_000_000_001),
            (i64::MAX - 1, i64::MAX),
            (0, i64::MAX),
        ] {
            let region = PracticeLoop::new(start(first), start(last)).unwrap();
            assert_eq!(region.start().nanoseconds(), first);
            assert_eq!(region.end().nanoseconds(), last);
            assert!(!region.reached(Timestamp::from_nanos(last - 1)));
            assert!(region.reached(Timestamp::from_nanos(last)));
        }
        assert!(PracticeLoop::new(start(i64::MAX), start(i64::MAX)).is_err());
    }
    #[test]
    fn frame_end_ceil_applies_original_start_and_preroll_once() {
        let region =
            PracticeLoop::new(start(604_800_000_000_000), start(604_800_001_000_001)).unwrap();
        for (rate, expected) in [(44_100, 45), (48_000, 49)] {
            assert_eq!(
                region
                    .playback_end_frame(region.start(), Duration::ZERO, rate)
                    .unwrap(),
                expected
            );
            assert_eq!(
                region
                    .playback_end_frame(region.start(), Duration::from_nanos(1_000_000_000), rate)
                    .unwrap(),
                expected + u64::from(rate)
            );
        }
        let exact =
            PracticeLoop::new(start(72_000_000_000_000), start(72_000_001_000_000)).unwrap();
        assert_eq!(
            exact
                .playback_end_frame(exact.start(), Duration::ZERO, 48_000)
                .unwrap(),
            48
        );
        let subframe = PracticeLoop::new(start(i64::MAX - 1), start(i64::MAX)).unwrap();
        assert_eq!(
            subframe
                .playback_end_frame(subframe.start(), Duration::ZERO, 48_000)
                .unwrap(),
            1
        );
    }
    #[test]
    fn invalid_frame_mapping_and_extreme_extent_fail_without_changing_region() {
        let region = PracticeLoop::new(start(0), start(i64::MAX)).unwrap();
        let before = region;
        assert!(
            region
                .playback_end_frame(start(1), Duration::ZERO, 48_000)
                .is_err()
        );
        assert!(
            region
                .playback_end_frame(start(0), Duration::from_nanos(-1), 48_000)
                .is_err()
        );
        assert!(
            region
                .playback_end_frame(start(0), Duration::ZERO, 0)
                .is_err()
        );
        assert!(
            region
                .playback_end_frame(start(0), Duration::from_nanos(i64::MAX), u32::MAX)
                .is_err()
        );
        assert_eq!(
            region
                .playback_end_frame(start(0), Duration::ZERO, 1)
                .unwrap(),
            9_223_372_037
        );
        assert_eq!(region, before);
    }
}
