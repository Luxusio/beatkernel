//! Exact observed practice endpoints; native restart ownership stays external.
use crate::practice::PracticeStart;
use beatkernel::time::Timestamp;

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
}
