//! Deferred deterministic start waiting; no native clock, socket or UI owner.
use crate::{
    competition_start_gate::{
        CompetitionSetupControl, CompetitionStartPort, StartGateResult, await_start,
        start_release_due,
    },
    multiplayer::MultiplayerError,
    multiplayer_start::StartSchedule,
    native_pump_control::NativePumpControl,
};
use std::{cell::RefCell, collections::VecDeque, rc::Rc, time::Duration};

type Trace = Rc<RefCell<Vec<&'static str>>>;
#[derive(Debug)]
struct Fault(&'static str);
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Fault {}
fn fault(label: &'static str) -> Box<dyn std::error::Error> {
    Box::new(Fault(label))
}
fn schedule(target: i64) -> StartSchedule {
    StartSchedule {
        target_ns: target,
        song_target_ns: target,
        uncertainty_ns: 0,
    }
}
struct Network {
    trace: Trace,
    ready_error: Option<Box<dyn std::error::Error>>,
    poll_error: Option<Box<dyn std::error::Error>>,
    poll_failure_on: Option<usize>,
    schedules: VecDeque<Option<StartSchedule>>,
    accepted: Option<StartSchedule>,
    releases: RefCell<VecDeque<StartGateResult<i64>>>,
    lateness: u64,
    ready: usize,
    polls: usize,
}
impl Network {
    fn new(trace: Trace, schedules: impl IntoIterator<Item = Option<StartSchedule>>) -> Self {
        Self {
            trace,
            ready_error: None,
            poll_error: None,
            poll_failure_on: None,
            schedules: schedules.into_iter().collect(),
            accepted: None,
            releases: RefCell::new(VecDeque::new()),
            lateness: 25,
            ready: 0,
            polls: 0,
        }
    }
}
impl CompetitionStartPort for Network {
    fn try_ready(&mut self) -> StartGateResult<()> {
        self.trace.borrow_mut().push("ready");
        self.ready += 1;
        match self.ready_error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    fn poll(&mut self) -> StartGateResult<()> {
        self.trace.borrow_mut().push("poll");
        self.polls += 1;
        if let Some(error) = self.poll_error.take() {
            return Err(error);
        }
        if self.poll_failure_on == Some(self.polls) {
            return Err(fault("post-deadline disconnect"));
        }
        if let Some(next) = self.schedules.pop_front() {
            self.accepted = next;
        }
        Ok(())
    }
    fn start_schedule(&self) -> Option<StartSchedule> {
        self.trace.borrow_mut().push("schedule");
        self.accepted
    }
    fn release_clock_now_ns(&self) -> StartGateResult<i64> {
        self.trace.borrow_mut().push("release");
        self.releases
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Err(fault("unexpected release read")))
    }
    fn max_release_lateness_ns(&self) -> u64 {
        self.trace.borrow_mut().push("lateness");
        self.lateness
    }
}
struct Control {
    trace: Trace,
    readings: VecDeque<StartGateResult<u64>>,
    waits: Vec<Duration>,
    wait_error: Option<Box<dyn std::error::Error>>,
}
impl Control {
    fn new(trace: Trace, readings: impl IntoIterator<Item = u64>) -> Self {
        Self {
            trace,
            readings: readings.into_iter().map(Ok).collect(),
            waits: vec![],
            wait_error: None,
        }
    }
}
impl NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> StartGateResult<u64> {
        self.trace.borrow_mut().push("control");
        self.readings
            .pop_front()
            .unwrap_or_else(|| Err(fault("unexpected control read")))
    }
    fn checked_add(moment: u64, duration: Duration) -> Option<u64> {
        moment.checked_add(u64::try_from(duration.as_nanos()).ok()?)
    }
    fn wait(&mut self, duration: Duration) -> StartGateResult<()> {
        self.trace.borrow_mut().push("wait");
        self.waits.push(duration);
        match self.wait_error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
impl CompetitionSetupControl for Control {
    fn remaining_duration(deadline: u64, now: u64) -> Duration {
        Duration::from_nanos(deadline.saturating_sub(now))
    }
}
fn service(trace: &Trace) -> impl FnMut() -> StartGateResult<bool> + '_ {
    || {
        trace.borrow_mut().push("service");
        Ok(true)
    }
}
fn assert_fault(error: &(dyn std::error::Error + 'static), expected: &'static str) {
    assert_eq!(error.downcast_ref::<Fault>().unwrap().0, expected);
}
fn assert_timeout(error: &(dyn std::error::Error + 'static)) {
    assert!(matches!(
        error.downcast_ref::<MultiplayerError>(),
        Some(MultiplayerError::SetupTimeout)
    ));
}

#[test]
fn commit_mode_admits_readiness_once_and_never_queries_release_clock() {
    let trace = Trace::default();
    let mut network = Network::new(trace.clone(), [Some(schedule(i64::MAX))]);
    let mut control = Control::new(trace.clone(), [17, 18]);
    assert!(
        await_start(
            &mut network,
            &mut control,
            Duration::from_nanos(100),
            false,
            service(&trace)
        )
        .unwrap()
    );
    assert_eq!(
        &*trace.borrow(),
        &["control", "ready", "service", "poll", "control", "schedule"]
    );
    assert_eq!((network.ready, network.polls), (1, 1));
    assert!(network.releases.borrow().is_empty());
    assert!(control.waits.is_empty());
    assert_eq!(network.accepted, Some(schedule(i64::MAX)));
}

#[test]
fn future_release_waits_for_original_network_clock_and_is_repeatable() {
    let mut prior = None;
    for _ in 0..2 {
        let trace = Trace::default();
        let mut network = Network::new(
            trace.clone(),
            [None, Some(schedule(100)), Some(schedule(100))],
        );
        network.releases.borrow_mut().extend([Ok(99), Ok(100)]);
        // These control values are deliberately unrelated to the release clock.
        let mut control = Control::new(
            trace.clone(),
            [
                1_000_000_000,
                1_000_000_000,
                1_000_000_001,
                1_005_000_000,
                1_005_000_001,
                1_010_000_000,
            ],
        );
        assert!(
            await_start(
                &mut network,
                &mut control,
                Duration::from_millis(50),
                true,
                service(&trace)
            )
            .unwrap()
        );
        assert_eq!(
            control.waits,
            [Duration::from_millis(5), Duration::from_millis(5)]
        );
        assert_eq!((network.ready, network.polls), (1, 3));
        let expected = vec![
            "control", "ready", "service", "poll", "control", "schedule", "control", "wait",
            "service", "poll", "control", "schedule", "release", "lateness", "control", "wait",
            "service", "poll", "control", "schedule", "release", "lateness",
        ];
        assert_eq!(*trace.borrow(), expected);
        if let Some(old) = &prior {
            assert_eq!(&expected, old);
        }
        prior = Some(expected);
    }
}

#[test]
fn release_bounds_are_inclusive_and_negative_or_excess_lateness_is_protocol_error() {
    for (target, now, bound, expected) in [
        (100, 99, 0, false),
        (100, 100, 0, true),
        (100, 125, 25, true),
        (i64::MAX, i64::MAX, 0, true),
        (0, i64::MAX, u64::MAX, true),
    ] {
        assert_eq!(
            start_release_due(schedule(target), now, bound).unwrap(),
            expected
        );
    }
    for (target, now, bound) in [
        (100, 126, 25),
        (-1, 0, 0),
        (0, -1, u64::MAX),
        (i64::MIN, i64::MAX, u64::MAX),
    ] {
        let error = start_release_due(schedule(target), now, bound).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<MultiplayerError>(),
            Some(MultiplayerError::Protocol(_))
        ));
    }
}

#[test]
fn cancellation_and_original_readiness_service_poll_errors_preserve_order_and_identity() {
    for case in 0..4 {
        let trace = Trace::default();
        let mut network = Network::new(trace.clone(), [Some(schedule(0))]);
        let mut control = Control::new(trace.clone(), [0, 100]);
        if case == 1 {
            network.ready_error = Some(fault("ready refusal"));
        }
        if case == 3 {
            network.poll_error = Some(fault("poll disconnect"));
        }
        let result = await_start(
            &mut network,
            &mut control,
            Duration::from_nanos(100),
            true,
            || {
                trace.borrow_mut().push("service");
                match case {
                    0 => Ok(false),
                    2 => Err(fault("service acquisition")),
                    _ => Ok(true),
                }
            },
        );
        match case {
            0 => {
                assert!(!result.unwrap());
                assert_eq!(&*trace.borrow(), &["control", "ready", "service"]);
            }
            1 => {
                assert_fault(result.unwrap_err().as_ref(), "ready refusal");
                assert_eq!(&*trace.borrow(), &["control", "ready"]);
            }
            2 => {
                assert_fault(result.unwrap_err().as_ref(), "service acquisition");
                assert_eq!(&*trace.borrow(), &["control", "ready", "service"]);
            }
            _ => {
                assert_fault(result.unwrap_err().as_ref(), "poll disconnect");
                assert_eq!(&*trace.borrow(), &["control", "ready", "service", "poll"]);
            }
        }
        assert!(control.waits.is_empty());
        assert!(network.releases.borrow().is_empty());
    }
}

#[test]
fn exact_timeout_precedes_committed_schedule_and_zero_or_overflow_has_no_readiness() {
    let trace = Trace::default();
    let mut network = Network::new(trace.clone(), [Some(schedule(0))]);
    let mut control = Control::new(trace.clone(), [0, 100]);
    assert_timeout(
        await_start(
            &mut network,
            &mut control,
            Duration::from_nanos(100),
            false,
            service(&trace),
        )
        .unwrap_err()
        .as_ref(),
    );
    assert_eq!(
        &*trace.borrow(),
        &["control", "ready", "service", "poll", "control"]
    );
    for (timeout, initial, expected_reads) in [
        (Duration::ZERO, 0, 0),
        (Duration::from_nanos(1), u64::MAX, 1),
        (Duration::from_secs(u64::MAX), 0, 1),
    ] {
        let trace = Trace::default();
        let mut network = Network::new(trace.clone(), []);
        let mut control = Control::new(trace.clone(), [initial]);
        let error =
            await_start(&mut network, &mut control, timeout, true, service(&trace)).unwrap_err();
        if timeout.is_zero() {
            assert_timeout(error.as_ref());
        }
        assert_eq!(network.ready, 0);
        assert_eq!(network.polls, 0);
        assert_eq!(trace.borrow().len(), expected_reads);
        assert!(control.waits.is_empty());
    }
}

#[test]
fn pending_wait_is_bounded_by_five_milliseconds_and_remaining_deadline() {
    for (timeout, expected) in [(10_000_000, 5_000_000), (4_000_000, 4_000_000), (1, 1)] {
        let trace = Trace::default();
        let mut network = Network::new(trace.clone(), [None]);
        let mut control = Control::new(trace.clone(), [0, 0, 0]);
        control.wait_error = Some(fault("wait refusal"));
        assert_fault(
            await_start(
                &mut network,
                &mut control,
                Duration::from_nanos(timeout),
                false,
                service(&trace),
            )
            .unwrap_err()
            .as_ref(),
            "wait refusal",
        );
        assert_eq!(control.waits, [Duration::from_nanos(expected)]);
        assert_eq!(
            &*trace.borrow(),
            &[
                "control", "ready", "service", "poll", "control", "schedule", "control", "wait"
            ]
        );
    }
    let trace = Trace::default();
    let mut network = Network::new(trace.clone(), [None]);
    let mut control = Control::new(trace.clone(), [0, 1, 9]);
    control.wait_error = Some(fault("remaining wait"));
    assert_fault(
        await_start(
            &mut network,
            &mut control,
            Duration::from_nanos(10),
            false,
            service(&trace),
        )
        .unwrap_err()
        .as_ref(),
        "remaining wait",
    );
    assert_eq!(control.waits, [Duration::from_nanos(1)]);
}

#[test]
fn control_failure_and_regression_never_become_a_release_timestamp() {
    for stage in 0..3 {
        let trace = Trace::default();
        let mut network = Network::new(trace.clone(), [None]);
        let mut control = Control::new(trace.clone(), []);
        control.readings.extend((0..stage).map(|_| Ok(10)));
        control.readings.push_back(Err(fault("control refusal")));
        assert_fault(
            await_start(
                &mut network,
                &mut control,
                Duration::from_nanos(100),
                true,
                service(&trace),
            )
            .unwrap_err()
            .as_ref(),
            "control refusal",
        );
        assert!(control.waits.is_empty());
        assert!(!trace.borrow().contains(&"release"));
        assert_eq!(network.ready, usize::from(stage > 0));
    }
    for readings in [vec![10, 9], vec![10, 11, 10]] {
        let trace = Trace::default();
        let mut network = Network::new(trace.clone(), [None]);
        let mut control = Control::new(trace.clone(), readings);
        let error = await_start(
            &mut network,
            &mut control,
            Duration::from_nanos(100),
            true,
            service(&trace),
        )
        .unwrap_err();
        assert!(error.to_string().contains("regress"));
        assert!(control.waits.is_empty());
        assert!(!trace.borrow().contains(&"release"));
    }
}

#[test]
fn release_error_is_original_and_expired_remaining_wait_preserves_next_service_poll_precedence() {
    let trace = Trace::default();
    let mut network = Network::new(trace.clone(), [Some(schedule(0))]);
    network
        .releases
        .borrow_mut()
        .push_back(Err(fault("network clock refusal")));
    let mut control = Control::new(trace.clone(), [0, 1]);
    assert_fault(
        await_start(
            &mut network,
            &mut control,
            Duration::from_nanos(100),
            true,
            service(&trace),
        )
        .unwrap_err()
        .as_ref(),
        "network clock refusal",
    );
    assert_eq!(
        &*trace.borrow(),
        &[
            "control", "ready", "service", "poll", "control", "schedule", "release"
        ]
    );
    assert!(control.waits.is_empty());
    // Original policy performs a zero wait if the remaining-time read reaches
    // the deadline, then services cancellation and polls before checking timeout.
    for outcome in 0..4 {
        let trace = Trace::default();
        let mut network = Network::new(trace.clone(), [None]);
        network.poll_failure_on = (outcome == 3).then_some(2);
        let mut control = Control::new(trace.clone(), [0, 1, 100, 100]);
        let mut services = 0;
        let result = await_start(
            &mut network,
            &mut control,
            Duration::from_nanos(100),
            false,
            || {
                trace.borrow_mut().push("service");
                services += 1;
                match (services, outcome) {
                    (2, 1) => Ok(false),
                    (2, 2) => Err(fault("post-deadline acquisition")),
                    _ => Ok(true),
                }
            },
        );
        match outcome {
            0 => assert_timeout(result.unwrap_err().as_ref()),
            1 => assert!(!result.unwrap()),
            2 => assert_fault(result.unwrap_err().as_ref(), "post-deadline acquisition"),
            _ => assert_fault(result.unwrap_err().as_ref(), "post-deadline disconnect"),
        }
        assert_eq!(control.waits, [Duration::ZERO]);
        assert_eq!(network.ready, 1);
        let mut expected = vec![
            "control", "ready", "service", "poll", "control", "schedule", "control", "wait",
            "service",
        ];
        if outcome == 0 {
            expected.extend(["poll", "control"]);
        }
        if outcome == 3 {
            expected.push("poll");
        }
        assert_eq!(*trace.borrow(), expected);
    }
}
