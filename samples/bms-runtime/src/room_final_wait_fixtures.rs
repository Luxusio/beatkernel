use super::*;
use crate::final_ack_wait::FinalWaitControl;
use std::{cell::RefCell, collections::VecDeque, rc::Rc, sync::Arc};

struct PortToken(Arc<u8>);
struct ControlToken(Arc<u8>);
#[derive(Debug, PartialEq, Eq)]
enum Effect {
    Poll,
    NetworkClock,
    ControlClock,
    Final,
    Drain,
    Park(u64),
}
type Trace = Rc<RefCell<Vec<Effect>>>;
struct Port {
    trace: Trace,
    polls: VecDeque<Result<RoomFinalObservation, PortToken>>,
    times: VecDeque<Result<i64, PortToken>>,
    finals: VecDeque<Result<RoomFinalAdmission, PortToken>>,
    drains: VecDeque<Result<RoomFinalAdmission, PortToken>>,
}
impl RoomFinalPort for Port {
    type Error = PortToken;
    fn poll(&mut self) -> Result<RoomFinalObservation, PortToken> {
        self.trace.borrow_mut().push(Effect::Poll);
        self.polls.pop_front().expect("unexpected room poll")
    }
    fn clock_now_ns(&mut self) -> Result<i64, PortToken> {
        self.trace.borrow_mut().push(Effect::NetworkClock);
        self.times.pop_front().expect("unexpected room clock")
    }
    fn queue_final(&mut self) -> Result<RoomFinalAdmission, PortToken> {
        self.trace.borrow_mut().push(Effect::Final);
        self.finals.pop_front().expect("unexpected final queue")
    }
    fn queue_drain(&mut self) -> Result<RoomFinalAdmission, PortToken> {
        self.trace.borrow_mut().push(Effect::Drain);
        self.drains.pop_front().expect("unexpected drain queue")
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
        self.trace.borrow_mut().push(Effect::ControlClock);
        self.times.pop_front().expect("unexpected control clock")
    }
    fn park_ns(&mut self, duration: u64) -> Result<(), ControlToken> {
        self.trace.borrow_mut().push(Effect::Park(duration));
        self.park_error.take().map_or(Ok(()), Err)
    }
}
fn observation(pending: bool, final_accepted: bool, drain_accepted: bool) -> RoomFinalObservation {
    RoomFinalObservation {
        progress_pending: pending,
        final_accepted,
        drain_accepted,
        terminal: None,
    }
}
fn receipts(mask: u8) -> RoomFinalReceipts {
    RoomFinalReceipts {
        local_final_written: mask & 1 != 0,
        local_final_acknowledged: mask & 2 != 0,
        progress_complete: mask & 4 != 0,
        drain_complete: mask & 8 != 0,
    }
}
fn terminal(
    mask: u8,
    cancelled: bool,
    failed: bool,
    final_accepted: bool,
    drain_accepted: bool,
) -> RoomFinalObservation {
    RoomFinalObservation {
        terminal: Some(RoomFinalTerminal {
            cancelled,
            failed,
            receipts: receipts(mask),
        }),
        ..observation(false, final_accepted, drain_accepted)
    }
}
fn setup(
    polls: impl IntoIterator<Item = Result<RoomFinalObservation, PortToken>>,
    network: impl IntoIterator<Item = Result<i64, PortToken>>,
    control: impl IntoIterator<Item = Result<u64, ControlToken>>,
    finals: impl IntoIterator<Item = Result<RoomFinalAdmission, PortToken>>,
    drains: impl IntoIterator<Item = Result<RoomFinalAdmission, PortToken>>,
) -> (Port, Control) {
    let trace = Rc::new(RefCell::new(vec![]));
    (
        Port {
            trace: trace.clone(),
            polls: polls.into_iter().collect(),
            times: network.into_iter().collect(),
            finals: finals.into_iter().collect(),
            drains: drains.into_iter().collect(),
        },
        Control {
            trace,
            times: control.into_iter().collect(),
            park_error: None,
        },
    )
}
fn first() -> Result<RoomFinalObservation, PortToken> {
    Ok(observation(false, false, false))
}
fn second() -> Result<RoomFinalObservation, PortToken> {
    Ok(observation(false, true, false))
}

#[test]
fn all_receipts_cancel_failure_and_observed_acceptance_gates_require_actual_own_admissions() {
    for mask in 0..16 {
        for cancelled in [false, true] {
            for failed in [false, true] {
                for final_accepted in [false, true] {
                    for drain_accepted in [false, true] {
                        let (mut port, mut control) = setup(
                            [
                                first(),
                                second(),
                                Ok(terminal(
                                    mask,
                                    cancelled,
                                    failed,
                                    final_accepted,
                                    drain_accepted,
                                )),
                            ],
                            [Ok(1), Ok(2), Ok(3)],
                            [Ok(10), Ok(11), Ok(12), Ok(13), Ok(14)],
                            [Ok(RoomFinalAdmission::Accepted)],
                            [Ok(RoomFinalAdmission::Accepted)],
                        );
                        let result = wait_for_room_final(&mut port, &mut control, 100, 10_000_000);
                        let valid =
                            mask == 15 && !cancelled && !failed && final_accepted && drain_accepted;
                        if valid {
                            assert!(result.is_ok());
                        } else {
                            assert!(matches!(result, Err(RoomFinalWaitError::InvalidTerminal)));
                        }
                        assert_eq!(
                            *port.trace.borrow(),
                            vec![
                                Effect::Poll,
                                Effect::NetworkClock,
                                Effect::ControlClock,
                                Effect::Final,
                                Effect::ControlClock,
                                Effect::Park(1_000_000),
                                Effect::Poll,
                                Effect::NetworkClock,
                                Effect::ControlClock,
                                Effect::Drain,
                                Effect::ControlClock,
                                Effect::Park(1_000_000),
                                Effect::Poll,
                                Effect::NetworkClock,
                                Effect::ControlClock
                            ]
                        );
                    }
                }
            }
        }
    }
    assert_eq!(RoomFinalReceipts::default(), receipts(0));
}

#[test]
fn early_complete_terminal_without_own_queue_intents_is_not_success_and_sends_nothing() {
    let (mut port, mut control) = setup(
        [Ok(terminal(15, false, false, true, true))],
        [Ok(1)],
        [Ok(10)],
        [],
        [],
    );
    assert!(matches!(
        wait_for_room_final(&mut port, &mut control, 100, 100),
        Err(RoomFinalWaitError::InvalidTerminal)
    ));
    assert_eq!(
        *port.trace.borrow(),
        vec![Effect::Poll, Effect::NetworkClock, Effect::ControlClock]
    );
}

#[test]
fn pending_progress_and_queue_pressure_preserve_order_and_never_repeat_accepted_commands() {
    let polls = [
        Ok(observation(true, false, false)),
        first(),
        first(),
        second(),
        second(),
        second(),
        Ok(terminal(15, false, false, true, true)),
    ];
    let (mut port, mut control) = setup(
        polls,
        (1..=7).map(Ok),
        (1..=13).map(Ok),
        [
            Ok(RoomFinalAdmission::QueueFull),
            Ok(RoomFinalAdmission::Accepted),
        ],
        [
            Ok(RoomFinalAdmission::QueueFull),
            Ok(RoomFinalAdmission::Accepted),
        ],
    );
    assert!(wait_for_room_final(&mut port, &mut control, 100, 20_000_000).is_ok());
    assert_eq!(
        *port.trace.borrow(),
        vec![
            Effect::Poll,
            Effect::NetworkClock,
            Effect::ControlClock,
            Effect::ControlClock,
            Effect::Park(1_000_000),
            Effect::Poll,
            Effect::NetworkClock,
            Effect::ControlClock,
            Effect::Final,
            Effect::ControlClock,
            Effect::Park(1_000_000),
            Effect::Poll,
            Effect::NetworkClock,
            Effect::ControlClock,
            Effect::Final,
            Effect::ControlClock,
            Effect::Park(1_000_000),
            Effect::Poll,
            Effect::NetworkClock,
            Effect::ControlClock,
            Effect::Drain,
            Effect::ControlClock,
            Effect::Park(1_000_000),
            Effect::Poll,
            Effect::NetworkClock,
            Effect::ControlClock,
            Effect::Drain,
            Effect::ControlClock,
            Effect::Park(1_000_000),
            Effect::Poll,
            Effect::NetworkClock,
            Effect::ControlClock,
            Effect::ControlClock,
            Effect::Park(1_000_000),
            Effect::Poll,
            Effect::NetworkClock,
            Effect::ControlClock
        ]
    );
}

#[test]
fn fixed_independent_deadlines_precede_even_complete_receipts_at_exact_expiry() {
    for network_expired in [false, true] {
        let (mut port, mut control) = setup(
            [
                first(),
                second(),
                Ok(terminal(15, false, false, true, true)),
            ],
            [Ok(1), Ok(2), Ok(3)],
            [Ok(10), Ok(11), Ok(12), Ok(13), Ok(14)],
            [Ok(RoomFinalAdmission::Accepted)],
            [Ok(RoomFinalAdmission::Accepted)],
        );
        assert!(matches!(
            wait_for_room_final(
                &mut port,
                &mut control,
                if network_expired { 3 } else { 100 },
                if network_expired { 10_000_000 } else { 14 }
            ),
            Err(RoomFinalWaitError::TimedOut)
        ));
        assert_eq!(
            port.trace
                .borrow()
                .iter()
                .filter(|e| matches!(e, Effect::Final))
                .count(),
            1
        );
        assert_eq!(
            port.trace
                .borrow()
                .iter()
                .filter(|e| matches!(e, Effect::Drain))
                .count(),
            1
        );
    }
}

#[test]
fn signed_network_and_unsigned_control_extents_remain_exact_near_their_distinct_maxima() {
    for (network, control) in [
        (604_800_000_000_001, 9_007_199_254_740_993),
        (9_007_199_254_740_993, u64::MAX - 10),
        (i64::MAX - 10, u64::MAX - 10),
    ] {
        let (mut port, mut clock) = setup(
            [
                first(),
                second(),
                Ok(terminal(15, false, false, true, true)),
            ],
            [Ok(network), Ok(network + 1), Ok(network + 2)],
            [
                Ok(control),
                Ok(control + 1),
                Ok(control + 2),
                Ok(control + 3),
                Ok(control + 4),
            ],
            [Ok(RoomFinalAdmission::Accepted)],
            [Ok(RoomFinalAdmission::Accepted)],
        );
        assert!(wait_for_room_final(&mut port, &mut clock, network + 10, control + 10).is_ok());
        assert!(port.trace.borrow().contains(&Effect::Park(9)));
        assert!(port.trace.borrow().contains(&Effect::Park(7)));
    }
    let (mut port, mut control) = setup(
        [Ok(terminal(15, false, false, true, true))],
        [Ok(i64::MAX)],
        [Ok(u64::MAX)],
        [],
        [],
    );
    assert!(matches!(
        wait_for_room_final(&mut port, &mut control, i64::MAX, u64::MAX),
        Err(RoomFinalWaitError::TimedOut)
    ));
}

#[test]
fn each_opaque_port_or_control_refusal_preserves_identity_and_prior_queue_effects() {
    for stage in 0..7 {
        let token = Arc::new(71);
        let polls = if stage == 0 {
            vec![Err(PortToken(token.clone()))]
        } else if stage == 4 {
            vec![first(), second()]
        } else {
            vec![first()]
        };
        let network = if stage == 1 {
            vec![Err(PortToken(token.clone()))]
        } else if stage == 0 {
            vec![]
        } else if stage == 4 {
            vec![Ok(1), Ok(2)]
        } else {
            vec![Ok(1)]
        };
        let times = match stage {
            0 | 1 => vec![],
            2 => vec![Err(ControlToken(token.clone()))],
            4 => vec![Ok(10), Ok(11), Ok(12)],
            5 => vec![Ok(10), Err(ControlToken(token.clone()))],
            _ => vec![Ok(10), Ok(11)],
        };
        let finals = if stage == 3 {
            vec![Err(PortToken(token.clone()))]
        } else if stage >= 4 {
            vec![Ok(RoomFinalAdmission::Accepted)]
        } else {
            vec![]
        };
        let drains = if stage == 4 {
            vec![Err(PortToken(token.clone()))]
        } else {
            vec![]
        };
        let (mut port, mut control) = setup(polls, network, times, finals, drains);
        if stage == 6 {
            control.park_error = Some(ControlToken(token.clone()));
        }
        match wait_for_room_final(&mut port, &mut control, 100, 10_000_000) {
            Err(RoomFinalWaitError::Port(PortToken(actual))) if matches!(stage, 0 | 1 | 3 | 4) => {
                assert!(Arc::ptr_eq(&actual, &token))
            }
            Err(RoomFinalWaitError::Control(ControlToken(actual)))
                if matches!(stage, 2 | 5 | 6) =>
            {
                assert!(Arc::ptr_eq(&actual, &token))
            }
            _ => panic!("failure must preserve its original opaque port/control token"),
        }
        let expected = match stage {
            0 => vec![Effect::Poll],
            1 => vec![Effect::Poll, Effect::NetworkClock],
            2 => vec![Effect::Poll, Effect::NetworkClock, Effect::ControlClock],
            3 => vec![
                Effect::Poll,
                Effect::NetworkClock,
                Effect::ControlClock,
                Effect::Final,
            ],
            4 => vec![
                Effect::Poll,
                Effect::NetworkClock,
                Effect::ControlClock,
                Effect::Final,
                Effect::ControlClock,
                Effect::Park(1_000_000),
                Effect::Poll,
                Effect::NetworkClock,
                Effect::ControlClock,
                Effect::Drain,
            ],
            5 => vec![
                Effect::Poll,
                Effect::NetworkClock,
                Effect::ControlClock,
                Effect::Final,
                Effect::ControlClock,
            ],
            _ => vec![
                Effect::Poll,
                Effect::NetworkClock,
                Effect::ControlClock,
                Effect::Final,
                Effect::ControlClock,
                Effect::Park(1_000_000),
            ],
        };
        assert_eq!(*port.trace.borrow(), expected);
    }
}

#[test]
fn negative_network_time_and_network_or_control_regressions_refuse_without_rollback() {
    for case in 0..4 {
        let polls = if case == 0 || case == 3 {
            vec![first()]
        } else {
            vec![first(), second()]
        };
        let network = match case {
            0 => vec![Ok(-1)],
            1 => vec![Ok(2), Ok(1)],
            _ => vec![Ok(1), Ok(2)],
        };
        let times = match case {
            0 => vec![],
            1 => vec![Ok(100), Ok(120)],
            2 => vec![Ok(100), Ok(120), Ok(110)],
            _ => vec![Ok(100), Ok(99)],
        };
        let finals = if case == 0 {
            vec![]
        } else {
            vec![Ok(RoomFinalAdmission::Accepted)]
        };
        let (mut port, mut control) = setup(polls, network, times, finals, []);
        let result = wait_for_room_final(&mut port, &mut control, 100, 10_000_000);
        if case == 0 {
            assert!(matches!(result, Err(RoomFinalWaitError::InvalidClock)));
        } else {
            assert!(matches!(result, Err(RoomFinalWaitError::ClockRegressed)));
        }
        assert_eq!(
            port.trace
                .borrow()
                .iter()
                .filter(|e| matches!(e, Effect::Final))
                .count(),
            usize::from(case != 0)
        );
        assert!(!port.trace.borrow().contains(&Effect::Drain));
    }
}

#[test]
fn fresh_post_queue_clock_clamps_park_to_remaining_time_or_zero_then_checks_timeout() {
    for fresh in [99, 100, 101] {
        let (mut port, mut control) = setup(
            [first(), Ok(terminal(15, false, false, true, true))],
            [Ok(1), Ok(2)],
            [Ok(90), Ok(fresh), Ok(fresh.max(100))],
            [Ok(RoomFinalAdmission::Accepted)],
            [],
        );
        assert!(matches!(
            wait_for_room_final(&mut port, &mut control, 100, 100),
            Err(RoomFinalWaitError::TimedOut)
        ));
        assert_eq!(
            *port.trace.borrow(),
            vec![
                Effect::Poll,
                Effect::NetworkClock,
                Effect::ControlClock,
                Effect::Final,
                Effect::ControlClock,
                Effect::Park(if fresh == 99 { 1 } else { 0 }),
                Effect::Poll,
                Effect::NetworkClock,
                Effect::ControlClock
            ]
        );
    }
}
