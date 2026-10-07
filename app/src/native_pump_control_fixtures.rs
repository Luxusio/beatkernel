// Deferred virtual control boundary. No system adapter or real wait is used.
use super::*;
use crate::native_gameplay::NativeGameplayResult;
use std::{collections::VecDeque, time::Duration as WaitDuration};

#[derive(Debug, PartialEq, Eq)]
struct Fault(&'static str);
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Fault {}
struct Script {
    readings: VecDeque<Result<u64, Fault>>,
    reads: usize,
    waits: Vec<WaitDuration>,
    fail_wait: bool,
}
impl Script {
    fn new(readings: impl IntoIterator<Item = u64>) -> Self {
        Self {
            readings: readings.into_iter().map(Ok).collect(),
            reads: 0,
            waits: Vec::new(),
            fail_wait: false,
        }
    }
}
impl NativePumpControl for Script {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        self.reads += 1;
        Ok(self
            .readings
            .pop_front()
            .expect("unexpected control-clock read")?)
    }
    fn checked_add(moment: u64, duration: WaitDuration) -> Option<u64> {
        moment.checked_add(u64::try_from(duration.as_nanos()).ok()?)
    }
    fn wait(&mut self, duration: WaitDuration) -> NativeGameplayResult<()> {
        self.waits.push(duration);
        if self.fail_wait {
            Err(Box::new(Fault("wait fault")))
        } else {
            Ok(())
        }
    }
}

#[test]
fn diagnostic_deadline_is_exclusive_checked_and_unlimited_never_reads_control_time() {
    let mut none = Script::new([]);
    let mut unlimited = NativePumpDeadline::new(&mut none, None, "deadline overflow").unwrap();
    for _ in 0..3 {
        assert!(unlimited.active(&mut none).unwrap());
    }
    none.wait(WaitDuration::from_millis(1)).unwrap();
    assert_eq!(none.reads, 0);
    assert_eq!(none.waits, [WaitDuration::from_millis(1)]);
    let mut zero = Script::new([7, 7]);
    let mut deadline = NativePumpDeadline::new(&mut zero, Some(0), "deadline overflow").unwrap();
    assert!(!deadline.active(&mut zero).unwrap());
    assert_eq!(zero.reads, 2);
    let mut exact = Script::new([5, 5, 1_000_000_004, 1_000_000_005, 1_000_000_006]);
    let mut deadline = NativePumpDeadline::new(&mut exact, Some(1), "deadline overflow").unwrap();
    assert!(deadline.active(&mut exact).unwrap());
    assert!(deadline.active(&mut exact).unwrap());
    assert!(!deadline.active(&mut exact).unwrap());
    assert!(!deadline.active(&mut exact).unwrap());
    assert_eq!(exact.reads, 5);
    let mut overflow = Script::new([u64::MAX]);
    let error = NativePumpDeadline::new(&mut overflow, Some(1), "literal overflow sentinel")
        .err()
        .unwrap();
    assert_eq!(error.to_string(), "literal overflow sentinel");
    assert_eq!(overflow.reads, 1);
}

#[test]
fn regression_and_original_clock_or_wait_faults_are_observable_without_hidden_effects() {
    let mut regressing = Script::new([100, 110, 109]);
    let mut deadline = NativePumpDeadline::new(&mut regressing, Some(1), "overflow").unwrap();
    assert!(deadline.active(&mut regressing).unwrap());
    assert!(deadline.active(&mut regressing).is_err());
    assert!(regressing.waits.is_empty());
    let mut initial = Script::new([]);
    initial
        .readings
        .push_back(Err(Fault("initial clock fault")));
    let error = NativePumpDeadline::new(&mut initial, Some(1), "overflow")
        .err()
        .unwrap();
    assert_eq!(
        error.downcast_ref::<Fault>(),
        Some(&Fault("initial clock fault"))
    );
    let mut later = Script::new([0]);
    later.readings.push_back(Err(Fault("later clock fault")));
    let mut deadline = NativePumpDeadline::new(&mut later, Some(1), "overflow").unwrap();
    let error = deadline.active(&mut later).unwrap_err();
    assert_eq!(
        error.downcast_ref::<Fault>(),
        Some(&Fault("later clock fault"))
    );
    assert_eq!(later.reads, 2);
    later.fail_wait = true;
    let error = later.wait(WaitDuration::from_millis(1)).unwrap_err();
    assert_eq!(error.downcast_ref::<Fault>(), Some(&Fault("wait fault")));
    assert_eq!(later.waits, [WaitDuration::from_millis(1)]);
}
