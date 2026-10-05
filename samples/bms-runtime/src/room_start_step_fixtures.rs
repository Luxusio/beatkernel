//! Deferred finite startup fixtures. Scripted ports own no OS resources.
use super::*;
use std::{cell::RefCell, collections::VecDeque, rc::Rc, sync::Arc};

// These associated errors intentionally provide no formatting/cloning traits.
struct PortError(Arc<u8>);
struct ServiceError(Arc<u8>);
struct ControlError(Arc<u8>);
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
    initial: Option<RoomStartInitial>,
    observations: VecDeque<RoomStartObservation<PortError>>,
}
impl RoomStartPort for Port {
    type Error = PortError;
    fn initial(&mut self) -> RoomStartInitial {
        self.trace.borrow_mut().push(Effect::Initial);
        self.initial
            .take()
            .expect("initial must be observed only once")
    }
    fn poll(&mut self) -> RoomStartObservation<PortError> {
        self.trace.borrow_mut().push(Effect::Poll);
        self.observations
            .pop_front()
            .expect("unexpected startup poll")
    }
}
struct Control {
    trace: Trace,
    error: Option<ControlError>,
}
impl RoomStartWaitControl for Control {
    type Error = ControlError;
    fn wait_ns(&mut self, ns: u64) -> Result<(), ControlError> {
        self.trace.borrow_mut().push(Effect::Wait(ns));
        self.error.take().map_or(Ok(()), Err)
    }
}
fn observation(committed: bool) -> RoomStartObservation<PortError> {
    RoomStartObservation {
        cancelled: false,
        failure: None,
        leaving: false,
        terminal: false,
        committed,
    }
}
fn port(initial: RoomStartInitial, observations: Vec<RoomStartObservation<PortError>>) -> Port {
    Port {
        trace: Rc::new(RefCell::new(Vec::new())),
        initial: Some(initial),
        observations: observations.into(),
    }
}
fn open(observations: Vec<RoomStartObservation<PortError>>) -> Port {
    port(
        RoomStartInitial {
            cancelled: false,
            closing: false,
        },
        observations,
    )
}
fn healthy(trace: Trace) -> impl FnMut() -> Result<bool, ServiceError> {
    move || {
        trace.borrow_mut().push(Effect::Service);
        Ok(true)
    }
}
fn sealed(state: &mut RoomStartWaitState, port: &mut Port) {
    let count = port.trace.borrow().len();
    let mut service = || -> Result<bool, ServiceError> { panic!("sealed state must not service") };
    assert!(matches!(
        state.step(port, &mut service),
        Err(RoomStartStepError::Terminal)
    ));
    assert_eq!(port.trace.borrow().len(), count);
}
fn repeated(state: &mut RoomStartWaitState, port: &mut Port, expected: RoomStartStep) {
    let count = port.trace.borrow().len();
    let mut service =
        || -> Result<bool, ServiceError> { panic!("terminal state must not service") };
    for _ in 0..3 {
        match state.step(port, &mut service) {
            Ok(value) => assert_eq!(value, expected),
            Err(_) => panic!("sticky successful terminal changed"),
        }
    }
    assert_eq!(port.trace.borrow().len(), count);
}

#[test]
fn all_initial_gates_run_once_and_cancel_precedes_closing() {
    for cancelled in [false, true] {
        for closing in [false, true] {
            let mut port = port(
                RoomStartInitial { cancelled, closing },
                vec![observation(true)],
            );
            let mut service = healthy(port.trace.clone());
            let mut state = RoomStartWaitState::new();
            let result = state.step(&mut port, &mut service);
            if cancelled {
                assert!(matches!(result, Ok(RoomStartStep::Cancelled)));
                assert_eq!(*port.trace.borrow(), [Effect::Initial]);
                repeated(&mut state, &mut port, RoomStartStep::Cancelled);
            } else if closing {
                assert!(matches!(result, Err(RoomStartStepError::Closing)));
                assert_eq!(*port.trace.borrow(), [Effect::Initial]);
                sealed(&mut state, &mut port);
            } else {
                assert!(matches!(result, Ok(RoomStartStep::Ready)));
                assert_eq!(
                    *port.trace.borrow(),
                    [Effect::Initial, Effect::Service, Effect::Poll]
                );
                repeated(&mut state, &mut port, RoomStartStep::Ready);
            }
        }
    }
}

#[test]
fn every_poll_precedence_combination_preserves_first_port_error_or_sticky_value() {
    for mask in 0..32 {
        let marker = Arc::new(mask);
        let cancelled = mask & 1 != 0;
        let failure = mask & 2 != 0;
        let leaving = mask & 4 != 0;
        let terminal = mask & 8 != 0;
        let committed = mask & 16 != 0;
        let mut port = open(vec![
            RoomStartObservation {
                cancelled,
                failure: failure.then(|| PortError(marker.clone())),
                leaving,
                terminal,
                committed,
            },
            observation(true),
        ]);
        let mut service = healthy(port.trace.clone());
        let mut state = RoomStartWaitState::new();
        let result = state.step(&mut port, &mut service);
        assert_eq!(
            *port.trace.borrow(),
            [Effect::Initial, Effect::Service, Effect::Poll]
        );
        if cancelled {
            assert!(matches!(result, Ok(RoomStartStep::Cancelled)));
            repeated(&mut state, &mut port, RoomStartStep::Cancelled);
        } else if failure {
            match result {
                Err(RoomStartStepError::Port(error)) => assert!(Arc::ptr_eq(&error.0, &marker)),
                _ => panic!("original poll error lost precedence"),
            }
            sealed(&mut state, &mut port);
        } else if leaving {
            assert!(matches!(result, Err(RoomStartStepError::LeavePending)));
            sealed(&mut state, &mut port);
        } else if terminal {
            assert!(matches!(result, Err(RoomStartStepError::Terminal)));
            sealed(&mut state, &mut port);
        } else if committed {
            assert!(matches!(result, Ok(RoomStartStep::Ready)));
            repeated(&mut state, &mut port, RoomStartStep::Ready);
        } else {
            assert!(matches!(result, Ok(RoomStartStep::Pending(1_000_000))));
            assert!(matches!(
                state.step(&mut port, &mut service),
                Ok(RoomStartStep::Ready)
            ));
            assert_eq!(
                *port.trace.borrow(),
                [
                    Effect::Initial,
                    Effect::Service,
                    Effect::Poll,
                    Effect::Service,
                    Effect::Poll
                ]
            );
        }
    }
}

#[test]
fn finite_pending_repeats_service_then_poll_without_waiting_or_reobserving_initial() {
    let mut port = open(vec![
        observation(false),
        observation(false),
        observation(false),
        observation(true),
    ]);
    let mut service = healthy(port.trace.clone());
    let mut state = RoomStartWaitState::new();
    for _ in 0..3 {
        assert!(matches!(
            state.step(&mut port, &mut service),
            Ok(RoomStartStep::Pending(1_000_000))
        ));
    }
    assert!(matches!(
        state.step(&mut port, &mut service),
        Ok(RoomStartStep::Ready)
    ));
    assert_eq!(
        *port.trace.borrow(),
        [
            Effect::Initial,
            Effect::Service,
            Effect::Poll,
            Effect::Service,
            Effect::Poll,
            Effect::Service,
            Effect::Poll,
            Effect::Service,
            Effect::Poll
        ]
    );
    assert!(port.observations.is_empty());
    repeated(&mut state, &mut port, RoomStartStep::Ready);
}

#[test]
fn service_cancellation_and_original_error_stop_before_poll_even_after_pending() {
    for error in [false, true] {
        for after_pending in [false, true] {
            let marker = Arc::new(93);
            let mut port = open(vec![observation(false), observation(true)]);
            let trace = port.trace.clone();
            let mut outputs: VecDeque<Result<bool, ServiceError>> = VecDeque::new();
            if after_pending {
                outputs.push_back(Ok(true));
            }
            outputs.push_back(if error {
                Err(ServiceError(marker.clone()))
            } else {
                Ok(false)
            });
            let mut service = move || {
                trace.borrow_mut().push(Effect::Service);
                outputs.pop_front().expect("unexpected service invocation")
            };
            let mut state = RoomStartWaitState::new();
            if after_pending {
                assert!(matches!(
                    state.step(&mut port, &mut service),
                    Ok(RoomStartStep::Pending(1_000_000))
                ));
            }
            let result = state.step(&mut port, &mut service);
            if error {
                match result {
                    Err(RoomStartStepError::Service(value)) => {
                        assert!(Arc::ptr_eq(&value.0, &marker))
                    }
                    _ => panic!("original service error was replaced"),
                }
                sealed(&mut state, &mut port);
            } else {
                assert!(matches!(result, Ok(RoomStartStep::Cancelled)));
                repeated(&mut state, &mut port, RoomStartStep::Cancelled);
            }
            assert_eq!(port.observations.len(), if after_pending { 1 } else { 2 });
            assert_eq!(
                *port.trace.borrow(),
                if after_pending {
                    vec![
                        Effect::Initial,
                        Effect::Service,
                        Effect::Poll,
                        Effect::Service,
                    ]
                } else {
                    vec![Effect::Initial, Effect::Service]
                }
            );
        }
    }
}

#[test]
fn unsized_service_callback_works_across_finite_pending_and_poll_cancellation() {
    let mut cancelled = observation(true);
    cancelled.cancelled = true;
    cancelled.failure = Some(PortError(Arc::new(81)));
    let mut port = open(vec![observation(false), cancelled]);
    let mut callback = healthy(port.trace.clone());
    let service: &mut dyn FnMut() -> Result<bool, ServiceError> = &mut callback;
    let mut state = RoomStartWaitState::new();
    assert!(matches!(
        state.step(&mut port, service),
        Ok(RoomStartStep::Pending(1_000_000))
    ));
    assert!(matches!(
        state.step(&mut port, service),
        Ok(RoomStartStep::Cancelled)
    ));
    assert_eq!(
        *port.trace.borrow(),
        [
            Effect::Initial,
            Effect::Service,
            Effect::Poll,
            Effect::Service,
            Effect::Poll
        ]
    );
    repeated(&mut state, &mut port, RoomStartStep::Cancelled);
}

#[test]
fn blocking_wrapper_alone_waits_and_original_control_refusal_stops_future_effects() {
    for refusal in [false, true] {
        let marker = Arc::new(51);
        let mut port = open(vec![
            observation(false),
            observation(false),
            observation(true),
        ]);
        let mut service = healthy(port.trace.clone());
        let mut control = Control {
            trace: port.trace.clone(),
            error: if refusal {
                Some(ControlError(marker.clone()))
            } else {
                None
            },
        };
        let result = await_room_start(&mut port, &mut control, &mut service);
        if refusal {
            match result {
                Err(RoomStartWaitError::Control(error)) => assert!(Arc::ptr_eq(&error.0, &marker)),
                _ => panic!("wrapper replaced control refusal"),
            }
            assert_eq!(
                *port.trace.borrow(),
                [
                    Effect::Initial,
                    Effect::Service,
                    Effect::Poll,
                    Effect::Wait(1_000_000)
                ]
            );
            assert_eq!(port.observations.len(), 2);
        } else {
            assert!(matches!(result, Ok(true)));
            assert_eq!(
                *port.trace.borrow(),
                [
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
            assert!(port.observations.is_empty());
        }
    }
}
