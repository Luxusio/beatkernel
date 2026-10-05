use super::*;
use std::{cell::RefCell, collections::VecDeque, rc::Rc, sync::Arc};

// Distinct opaque error types; no Clone/Debug/Display/Error implementations.
struct PortToken(Arc<u8>);
struct ServiceToken(Arc<u8>);
struct WaitToken(Arc<u8>);
#[derive(Debug, PartialEq, Eq)]
enum Effect {
    Initial,
    Service,
    Poll,
    Wait(u64),
}
type Trace = Rc<RefCell<Vec<Effect>>>;
struct Port {
    trace: Trace,
    initial: RoomStartInitial,
    polls: VecDeque<RoomStartObservation<PortToken>>,
}
impl RoomStartPort for Port {
    type Error = PortToken;
    fn initial(&mut self) -> RoomStartInitial {
        self.trace.borrow_mut().push(Effect::Initial);
        RoomStartInitial {
            cancelled: self.initial.cancelled,
            closing: self.initial.closing,
        }
    }
    fn poll(&mut self) -> RoomStartObservation<PortToken> {
        self.trace.borrow_mut().push(Effect::Poll);
        self.polls.pop_front().expect("unexpected startup poll")
    }
}
struct Control {
    trace: Trace,
    error: Option<WaitToken>,
}
impl RoomStartWaitControl for Control {
    type Error = WaitToken;
    fn wait_ns(&mut self, duration: u64) -> Result<(), WaitToken> {
        self.trace.borrow_mut().push(Effect::Wait(duration));
        self.error.take().map_or(Ok(()), Err)
    }
}
fn pending() -> RoomStartObservation<PortToken> {
    RoomStartObservation {
        cancelled: false,
        failure: None,
        leaving: false,
        terminal: false,
        committed: false,
    }
}
fn committed() -> RoomStartObservation<PortToken> {
    RoomStartObservation {
        committed: true,
        ..pending()
    }
}
fn setup(polls: impl IntoIterator<Item = RoomStartObservation<PortToken>>) -> (Port, Control) {
    let trace = Rc::new(RefCell::new(vec![]));
    (
        Port {
            trace: trace.clone(),
            initial: RoomStartInitial {
                cancelled: false,
                closing: false,
            },
            polls: polls.into_iter().collect(),
        },
        Control { trace, error: None },
    )
}
fn service(
    trace: Trace,
    outcomes: impl IntoIterator<Item = Result<bool, ServiceToken>>,
) -> impl FnMut() -> Result<bool, ServiceToken> {
    let mut outcomes: VecDeque<_> = outcomes.into_iter().collect();
    move || {
        trace.borrow_mut().push(Effect::Service);
        outcomes.pop_front().expect("unexpected startup service")
    }
}

#[test]
fn every_initial_gate_pins_cancellation_before_closing_and_before_service_poll_or_wait() {
    for cancelled in [false, true] {
        for closing in [false, true] {
            let (mut port, mut control) = setup([committed()]);
            port.initial = RoomStartInitial { cancelled, closing };
            let mut service = service(port.trace.clone(), [Ok(true)]);
            let result = await_room_start(&mut port, &mut control, &mut service);
            if cancelled {
                assert!(matches!(result, Ok(false)));
            } else if closing {
                assert!(matches!(result, Err(RoomStartWaitError::Closing)));
            } else {
                assert!(matches!(result, Ok(true)));
            }
            assert_eq!(
                *port.trace.borrow(),
                if cancelled || closing {
                    vec![Effect::Initial]
                } else {
                    vec![Effect::Initial, Effect::Service, Effect::Poll]
                }
            );
        }
    }
}

#[test]
fn all_nonpending_poll_combinations_pin_cancel_failure_leave_terminal_commit_precedence() {
    for cancelled in [false, true] {
        for failed in [false, true] {
            for leaving in [false, true] {
                for terminal in [false, true] {
                    for committed in [false, true] {
                        if !cancelled && !failed && !leaving && !terminal && !committed {
                            continue;
                        }
                        let identity = Arc::new(31);
                        let observation = RoomStartObservation {
                            cancelled,
                            failure: failed.then(|| PortToken(identity.clone())),
                            leaving,
                            terminal,
                            committed,
                        };
                        let (mut port, mut control) = setup([observation]);
                        let mut service = service(port.trace.clone(), [Ok(true)]);
                        let result = await_room_start(&mut port, &mut control, &mut service);
                        if cancelled {
                            assert!(matches!(result, Ok(false)));
                        } else if failed {
                            match result {
                                Err(RoomStartWaitError::Port(PortToken(actual))) => {
                                    assert!(Arc::ptr_eq(&identity, &actual))
                                }
                                _ => panic!(
                                    "uncancelled poll failure must retain identity before any retained commit"
                                ),
                            }
                        } else if leaving {
                            assert!(matches!(result, Err(RoomStartWaitError::LeavePending)));
                        } else if terminal {
                            assert!(matches!(result, Err(RoomStartWaitError::Terminal)));
                        } else {
                            assert!(matches!(result, Ok(true)));
                        }
                        assert_eq!(
                            *port.trace.borrow(),
                            vec![Effect::Initial, Effect::Service, Effect::Poll]
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn original_service_error_and_service_cancellation_stop_before_poll_or_wait() {
    for error in [false, true] {
        let identity = Arc::new(53);
        let (mut port, mut control) = setup([committed()]);
        let mut service = service(
            port.trace.clone(),
            [if error {
                Err(ServiceToken(identity.clone()))
            } else {
                Ok(false)
            }],
        );
        let result = await_room_start(&mut port, &mut control, &mut service);
        if error {
            match result {
                Err(RoomStartWaitError::Service(ServiceToken(actual))) => {
                    assert!(Arc::ptr_eq(&actual, &identity))
                }
                _ => panic!("service error must remain its distinct original token"),
            }
        } else {
            assert!(matches!(result, Ok(false)));
        }
        assert_eq!(*port.trace.borrow(), vec![Effect::Initial, Effect::Service]);
        assert_eq!(port.polls.len(), 1);
    }
}

#[test]
fn repeated_pending_waits_exactly_one_millisecond_before_each_fresh_service_and_poll() {
    for outcome in 0..5 {
        let identity = Arc::new(71);
        let mut final_observation = committed();
        match outcome {
            1 => final_observation.cancelled = true,
            2 => final_observation.failure = Some(PortToken(identity.clone())),
            3 => final_observation.leaving = true,
            4 => final_observation.terminal = true,
            _ => {}
        }
        let (mut port, mut control) = setup([pending(), pending(), final_observation]);
        let mut service = service(port.trace.clone(), [Ok(true), Ok(true), Ok(true)]);
        let result = await_room_start(&mut port, &mut control, &mut service);
        match outcome {
            0 => assert!(matches!(result, Ok(true))),
            1 => assert!(matches!(result, Ok(false))),
            2 => match result {
                Err(RoomStartWaitError::Port(PortToken(actual))) => {
                    assert!(Arc::ptr_eq(&actual, &identity))
                }
                _ => panic!("late failure must not activate retained commit"),
            },
            3 => assert!(matches!(result, Err(RoomStartWaitError::LeavePending))),
            _ => assert!(matches!(result, Err(RoomStartWaitError::Terminal))),
        }
        assert_eq!(
            *port.trace.borrow(),
            vec![
                Effect::Initial,
                Effect::Service,
                Effect::Poll,
                Effect::Wait(1_000_000),
                Effect::Service,
                Effect::Poll,
                Effect::Wait(1_000_000),
                Effect::Service,
                Effect::Poll
            ]
        );
    }
    let (mut port, mut control) = setup([pending(), committed()]);
    let mut service = service(port.trace.clone(), [Ok(true), Ok(false)]);
    assert!(matches!(
        await_room_start(&mut port, &mut control, &mut service),
        Ok(false)
    ));
    assert_eq!(port.polls.len(), 1);
    assert_eq!(
        *port.trace.borrow(),
        vec![
            Effect::Initial,
            Effect::Service,
            Effect::Poll,
            Effect::Wait(1_000_000),
            Effect::Service
        ]
    );
}

#[test]
fn wait_refusal_retains_its_original_token_and_prevents_further_service_or_commit_poll() {
    let identity = Arc::new(83);
    let (mut port, mut control) = setup([pending(), committed()]);
    control.error = Some(WaitToken(identity.clone()));
    let mut service = service(port.trace.clone(), [Ok(true), Ok(true)]);
    match await_room_start(&mut port, &mut control, &mut service) {
        Err(RoomStartWaitError::Control(WaitToken(actual))) => {
            assert!(Arc::ptr_eq(&actual, &identity))
        }
        _ => panic!("wait refusal must retain its distinct original token"),
    }
    assert_eq!(port.polls.len(), 1);
    assert_eq!(
        *port.trace.borrow(),
        vec![
            Effect::Initial,
            Effect::Service,
            Effect::Poll,
            Effect::Wait(1_000_000)
        ]
    );
}

#[test]
fn borrowed_unsized_service_callback_uses_the_actual_policy() {
    let (mut port, mut control) = setup([pending(), committed()]);
    let mut closure = service(port.trace.clone(), [Ok(true), Ok(true)]);
    let callback: &mut dyn FnMut() -> Result<bool, ServiceToken> = &mut closure;
    assert!(matches!(
        await_room_start(&mut port, &mut control, callback),
        Ok(true)
    ));
    assert_eq!(
        *port.trace.borrow(),
        vec![
            Effect::Initial,
            Effect::Service,
            Effect::Poll,
            Effect::Wait(1_000_000),
            Effect::Service,
            Effect::Poll
        ]
    );
}
