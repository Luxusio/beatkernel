use super::*;
use std::{cell::RefCell, collections::VecDeque, rc::Rc, sync::Arc};

struct PortToken(Arc<u8>);
struct ControlToken(Arc<u8>); // Neither associated error has formatting/clone/error traits.
#[derive(Debug, PartialEq, Eq)]
enum Effect {
    Poll,
    Admit,
    Clock,
    Park(u64),
}
type Trace = Rc<RefCell<Vec<Effect>>>;
struct Port {
    trace: Trace,
    polls: VecDeque<Result<FinalAckObservation<PortToken>, PortToken>>,
    admissions: VecDeque<Result<FinalAdmission, PortToken>>,
}
impl FinalAckPort for Port {
    type Error = PortToken;
    fn poll(&mut self) -> Result<FinalAckObservation<PortToken>, PortToken> {
        self.trace.borrow_mut().push(Effect::Poll);
        self.polls
            .pop_front()
            .expect("unexpected notice batch poll")
    }
    fn admit(&mut self) -> Result<FinalAdmission, PortToken> {
        self.trace.borrow_mut().push(Effect::Admit);
        self.admissions
            .pop_front()
            .expect("unexpected terminal admission")
    }
}
struct Control {
    trace: Trace,
    times: VecDeque<Result<u64, ControlToken>>,
    park_error: Option<ControlToken>,
}
impl FinalWaitControl for Control {
    type Error = ControlToken;
    fn now_ns(&mut self) -> Result<u64, ControlToken> {
        self.trace.borrow_mut().push(Effect::Clock);
        self.times.pop_front().expect("unexpected wait clock read")
    }
    fn park_ns(&mut self, duration: u64) -> Result<(), ControlToken> {
        self.trace.borrow_mut().push(Effect::Park(duration));
        self.park_error.take().map_or(Ok(()), Err)
    }
}
fn pending() -> Result<FinalAckObservation<PortToken>, PortToken> {
    Ok(FinalAckObservation {
        cancelled: false,
        acknowledged: false,
        closed: false,
        failure: None,
    })
}
fn ack() -> Result<FinalAckObservation<PortToken>, PortToken> {
    Ok(FinalAckObservation {
        cancelled: false,
        acknowledged: true,
        closed: false,
        failure: None,
    })
}
fn setup(
    polls: impl IntoIterator<Item = Result<FinalAckObservation<PortToken>, PortToken>>,
    admissions: impl IntoIterator<Item = Result<FinalAdmission, PortToken>>,
    times: impl IntoIterator<Item = Result<u64, ControlToken>>,
) -> (Port, Control) {
    let trace = Rc::new(RefCell::new(vec![]));
    (
        Port {
            trace: trace.clone(),
            polls: polls.into_iter().collect(),
            admissions: admissions.into_iter().collect(),
        },
        Control {
            trace,
            times: times.into_iter().collect(),
            park_error: None,
        },
    )
}

#[test]
fn all_terminal_observation_combinations_pin_precedence_without_clock_or_admission() {
    for cancelled in [false, true] {
        for acknowledged in [false, true] {
            for closed in [false, true] {
                for kind in 0..3 {
                    if !cancelled && !acknowledged && !closed && kind == 0 {
                        continue;
                    }
                    let identity = Arc::new(31);
                    let failure = match kind {
                        0 => None,
                        1 => Some(FinalAckFailure::Closed(PortToken(identity.clone()))),
                        _ => Some(FinalAckFailure::Other(PortToken(identity.clone()))),
                    };
                    let observation = FinalAckObservation {
                        cancelled,
                        acknowledged,
                        closed,
                        failure,
                    };
                    let (mut port, mut control) = setup([Ok(observation)], [], []);
                    let result = wait_for_final_ack(&mut port, &mut control, 0, false);
                    if cancelled {
                        assert!(matches!(result, Err(FinalAckWaitError::Cancelled)));
                    } else if kind == 2 || (kind == 1 && !acknowledged) {
                        match result {
                            Err(FinalAckWaitError::Port(PortToken(actual))) => {
                                assert!(Arc::ptr_eq(&actual, &identity))
                            }
                            _ => panic!("non-tolerated original disconnect must dominate any ACK"),
                        }
                    } else if acknowledged {
                        assert!(result.is_ok());
                    } else {
                        assert!(matches!(result, Err(FinalAckWaitError::Closed)));
                    }
                    assert_eq!(*port.trace.borrow(), [Effect::Poll]);
                }
            }
        }
    }
}

#[test]
fn queue_full_retries_but_accepted_admission_occurs_only_once_and_never_creates_ack() {
    let (mut port, mut control) = setup(
        [pending(), pending(), pending(), pending(), ack()],
        [
            Ok(FinalAdmission::QueueFull),
            Ok(FinalAdmission::QueueFull),
            Ok(FinalAdmission::Accepted),
        ],
        [Ok(0), Ok(1), Ok(2), Ok(3), Ok(4), Ok(5), Ok(6), Ok(7)],
    );
    assert!(wait_for_final_ack(&mut port, &mut control, 20_000_000, false).is_ok());
    assert_eq!(
        *port.trace.borrow(),
        vec![
            Effect::Poll,
            Effect::Clock,
            Effect::Admit,
            Effect::Clock,
            Effect::Park(5_000_000),
            Effect::Poll,
            Effect::Clock,
            Effect::Admit,
            Effect::Clock,
            Effect::Park(5_000_000),
            Effect::Poll,
            Effect::Clock,
            Effect::Admit,
            Effect::Clock,
            Effect::Park(5_000_000),
            Effect::Poll,
            Effect::Clock,
            Effect::Clock,
            Effect::Park(5_000_000),
            Effect::Poll
        ]
    );
    assert!(port.admissions.is_empty());
}

#[test]
fn already_admitted_input_never_calls_admission_even_when_waiting_for_actual_ack() {
    let (mut port, mut control) = setup([pending(), ack()], [], [Ok(100), Ok(101)]);
    assert!(wait_for_final_ack(&mut port, &mut control, 200, true).is_ok());
    assert_eq!(
        *port.trace.borrow(),
        vec![
            Effect::Poll,
            Effect::Clock,
            Effect::Clock,
            Effect::Park(99),
            Effect::Poll
        ]
    );
}

#[test]
fn queue_pressure_keeps_the_original_deadline_and_exact_expiry_precedes_admission() {
    let (mut port, mut control) = setup(
        [pending(), pending(), pending()],
        [Ok(FinalAdmission::QueueFull), Ok(FinalAdmission::QueueFull)],
        [Ok(0), Ok(1), Ok(4_999_999), Ok(5_000_000), Ok(5_000_000)],
    );
    assert!(matches!(
        wait_for_final_ack(&mut port, &mut control, 5_000_000, false),
        Err(FinalAckWaitError::TimedOut)
    ));
    assert_eq!(
        *port.trace.borrow(),
        vec![
            Effect::Poll,
            Effect::Clock,
            Effect::Admit,
            Effect::Clock,
            Effect::Park(4_999_999),
            Effect::Poll,
            Effect::Clock,
            Effect::Admit,
            Effect::Clock,
            Effect::Park(0),
            Effect::Poll,
            Effect::Clock
        ]
    );
}

#[test]
fn exact_large_clock_values_preserve_week_and_above_float_precision_boundaries() {
    for origin in [604_800_000_000_001, 9_007_199_254_740_993, u64::MAX - 10] {
        let deadline = origin + 10;
        let (mut port, mut control) = setup(
            [pending(), ack()],
            [Ok(FinalAdmission::Accepted)],
            [Ok(origin), Ok(origin + 1)],
        );
        assert!(wait_for_final_ack(&mut port, &mut control, deadline, false).is_ok());
        assert_eq!(
            *port.trace.borrow(),
            vec![
                Effect::Poll,
                Effect::Clock,
                Effect::Admit,
                Effect::Clock,
                Effect::Park(9),
                Effect::Poll
            ]
        );
        let (mut port, mut control) = setup([pending()], [], [Ok(deadline)]);
        assert!(matches!(
            wait_for_final_ack(&mut port, &mut control, deadline, false),
            Err(FinalAckWaitError::TimedOut)
        ));
        assert_eq!(*port.trace.borrow(), vec![Effect::Poll, Effect::Clock]);
    }
    let (mut port, mut control) = setup([ack()], [], []);
    assert!(wait_for_final_ack(&mut port, &mut control, u64::MAX, false).is_ok());
    assert_eq!(*port.trace.borrow(), vec![Effect::Poll]);
}

#[test]
fn poll_admission_clock_and_park_errors_keep_original_distinct_opaque_tokens() {
    for stage in 0..5 {
        let identity = Arc::new(53);
        let polls = if stage == 0 {
            vec![Err(PortToken(identity.clone()))]
        } else {
            vec![pending()]
        };
        let admissions = if stage == 1 {
            vec![Err(PortToken(identity.clone()))]
        } else if stage >= 3 {
            vec![Ok(FinalAdmission::Accepted)]
        } else {
            vec![]
        };
        let times = match stage {
            0 => vec![],
            1 => vec![Ok(10)],
            2 => vec![Err(ControlToken(identity.clone()))],
            3 => vec![Ok(10), Err(ControlToken(identity.clone()))],
            _ => vec![Ok(10), Ok(11)],
        };
        let (mut port, mut control) = setup(polls, admissions, times);
        if stage == 4 {
            control.park_error = Some(ControlToken(identity.clone()));
        }
        match wait_for_final_ack(&mut port, &mut control, 100, false) {
            Err(FinalAckWaitError::Port(PortToken(actual))) if stage < 2 => {
                assert!(Arc::ptr_eq(&actual, &identity))
            }
            Err(FinalAckWaitError::Control(ControlToken(actual))) if stage >= 2 => {
                assert!(Arc::ptr_eq(&actual, &identity))
            }
            _ => panic!("each port/control refusal must retain its original opaque token"),
        }
        let expected = match stage {
            0 => vec![Effect::Poll],
            1 => vec![Effect::Poll, Effect::Clock, Effect::Admit],
            2 => vec![Effect::Poll, Effect::Clock],
            3 => vec![Effect::Poll, Effect::Clock, Effect::Admit, Effect::Clock],
            _ => vec![
                Effect::Poll,
                Effect::Clock,
                Effect::Admit,
                Effect::Clock,
                Effect::Park(89),
            ],
        };
        assert_eq!(*port.trace.borrow(), expected);
    }
}

#[test]
fn pre_and_post_clock_regression_refuse_without_undoing_prior_admission() {
    for post in [false, true] {
        let polls = if post {
            vec![pending()]
        } else {
            vec![pending(), pending()]
        };
        let times = if post {
            vec![Ok(100), Ok(99)]
        } else {
            vec![Ok(100), Ok(120), Ok(110)]
        };
        let (mut port, mut control) = setup(polls, [Ok(FinalAdmission::Accepted)], times);
        assert!(matches!(
            wait_for_final_ack(&mut port, &mut control, 10_000_000, false),
            Err(FinalAckWaitError::ClockRegressed)
        ));
        assert!(port.admissions.is_empty());
        let expected = if post {
            vec![Effect::Poll, Effect::Clock, Effect::Admit, Effect::Clock]
        } else {
            vec![
                Effect::Poll,
                Effect::Clock,
                Effect::Admit,
                Effect::Clock,
                Effect::Park(5_000_000),
                Effect::Poll,
                Effect::Clock,
            ]
        };
        assert_eq!(*port.trace.borrow(), expected);
    }
}

#[test]
fn post_admission_overshoot_parks_zero_then_next_poll_ack_can_win_over_timeout() {
    for acknowledged in [false, true] {
        let next = if acknowledged { ack() } else { pending() };
        let (mut port, mut control) = setup(
            [pending(), next],
            [Ok(FinalAdmission::Accepted)],
            [Ok(99), Ok(101), Ok(101)],
        );
        let result = wait_for_final_ack(&mut port, &mut control, 100, false);
        if acknowledged {
            assert!(result.is_ok());
        } else {
            assert!(matches!(result, Err(FinalAckWaitError::TimedOut)));
        }
        let mut expected = vec![
            Effect::Poll,
            Effect::Clock,
            Effect::Admit,
            Effect::Clock,
            Effect::Park(0),
            Effect::Poll,
        ];
        if !acknowledged {
            expected.push(Effect::Clock);
        }
        assert_eq!(*port.trace.borrow(), expected);
        assert!(port.admissions.is_empty());
    }
}
