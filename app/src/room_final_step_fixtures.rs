//! Deferred single-iteration final policy fixtures; no physical clock or IO.
use super::*;
use crate::final_ack_wait::FinalWaitControl;
use std::{cell::RefCell, collections::VecDeque, rc::Rc, sync::Arc};

// Deliberately no Clone, Debug, Display or Error implementations.
struct PortError(Arc<u8>);
struct ControlError(Arc<u8>);
#[derive(Debug, PartialEq, Eq)]
enum Effect {
    Poll,
    Network,
    Control,
    Final,
    Drain,
    Park(u64),
}
type Trace = Rc<RefCell<Vec<Effect>>>;
struct Port {
    trace: Trace,
    polls: VecDeque<Result<RoomFinalObservation, PortError>>,
    network: VecDeque<Result<i64, PortError>>,
    finals: VecDeque<Result<RoomFinalAdmission, PortError>>,
    drains: VecDeque<Result<RoomFinalAdmission, PortError>>,
}
impl RoomFinalPort for Port {
    type Error = PortError;
    fn poll(&mut self) -> Result<RoomFinalObservation, PortError> {
        self.trace.borrow_mut().push(Effect::Poll);
        self.polls.pop_front().expect("unexpected poll")
    }
    fn clock_now_ns(&mut self) -> Result<i64, PortError> {
        self.trace.borrow_mut().push(Effect::Network);
        self.network.pop_front().expect("unexpected network clock")
    }
    fn queue_final(&mut self) -> Result<RoomFinalAdmission, PortError> {
        self.trace.borrow_mut().push(Effect::Final);
        self.finals.pop_front().expect("unexpected final admission")
    }
    fn queue_drain(&mut self) -> Result<RoomFinalAdmission, PortError> {
        self.trace.borrow_mut().push(Effect::Drain);
        self.drains.pop_front().expect("unexpected drain admission")
    }
}
struct Control {
    trace: Trace,
    times: VecDeque<Result<u64, ControlError>>,
    allow_park: bool,
    park_error: Option<ControlError>,
}
impl FinalWaitControl for Control {
    type Error = ControlError;
    fn now_ns(&mut self) -> Result<u64, ControlError> {
        self.trace.borrow_mut().push(Effect::Control);
        self.times.pop_front().expect("unexpected control clock")
    }
    fn park_ns(&mut self, ns: u64) -> Result<(), ControlError> {
        assert!(self.allow_park, "direct state step must never park");
        self.trace.borrow_mut().push(Effect::Park(ns));
        self.park_error.take().map_or(Ok(()), Err)
    }
}
fn pending(
    progress_pending: bool,
    final_accepted: bool,
    drain_accepted: bool,
) -> RoomFinalObservation {
    RoomFinalObservation {
        progress_pending,
        final_accepted,
        drain_accepted,
        terminal: None,
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
            receipts: RoomFinalReceipts {
                local_final_written: mask & 1 != 0,
                local_final_acknowledged: mask & 2 != 0,
                progress_complete: mask & 4 != 0,
                drain_complete: mask & 8 != 0,
            },
        }),
        ..pending(false, final_accepted, drain_accepted)
    }
}
fn setup(
    polls: Vec<Result<RoomFinalObservation, PortError>>,
    network: Vec<Result<i64, PortError>>,
    times: Vec<Result<u64, ControlError>>,
    finals: Vec<Result<RoomFinalAdmission, PortError>>,
    drains: Vec<Result<RoomFinalAdmission, PortError>>,
) -> (Port, Control) {
    let trace = Rc::new(RefCell::new(Vec::new()));
    (
        Port {
            trace: trace.clone(),
            polls: polls.into(),
            network: network.into(),
            finals: finals.into(),
            drains: drains.into(),
        },
        Control {
            trace,
            times: times.into(),
            allow_park: false,
            park_error: None,
        },
    )
}
fn wait(state: &mut RoomFinalWaitState, port: &mut Port, control: &mut Control, expected: u64) {
    match state.step(port, control) {
        Ok(value) => assert_eq!(value, RoomFinalStep::Wait(expected)),
        Err(_) => panic!("unexpected direct-step refusal"),
    }
}
fn sealed(state: &mut RoomFinalWaitState, port: &mut Port, control: &mut Control) {
    let before = port.trace.borrow().len();
    assert!(matches!(
        state.step(port, control),
        Err(RoomFinalWaitError::InvalidTerminal)
    ));
    assert_eq!(port.trace.borrow().len(), before);
}

#[test]
fn queue_pressure_and_progress_gates_persist_once_only_admissions_between_steps() {
    let (mut port, mut control) = setup(
        vec![
            Ok(pending(true, false, false)),
            Ok(pending(false, false, false)),
            Ok(pending(false, false, false)),
            Ok(pending(false, true, false)),
            Ok(pending(false, true, false)),
            Ok(pending(false, true, true)),
            Ok(terminal(15, false, false, true, true)),
        ],
        (1..=7).map(Ok).collect(),
        (10..=22).map(Ok).collect(),
        vec![
            Ok(RoomFinalAdmission::QueueFull),
            Ok(RoomFinalAdmission::Accepted),
        ],
        vec![
            Ok(RoomFinalAdmission::QueueFull),
            Ok(RoomFinalAdmission::Accepted),
        ],
    );
    let mut state = RoomFinalWaitState::new(100, 10_000_000);
    for _ in 0..6 {
        wait(&mut state, &mut port, &mut control, 1_000_000);
    }
    assert!(matches!(
        state.step(&mut port, &mut control),
        Ok(RoomFinalStep::Completed)
    ));
    assert_eq!(
        *port.trace.borrow(),
        vec![
            Effect::Poll,
            Effect::Network,
            Effect::Control,
            Effect::Control,
            Effect::Poll,
            Effect::Network,
            Effect::Control,
            Effect::Final,
            Effect::Control,
            Effect::Poll,
            Effect::Network,
            Effect::Control,
            Effect::Final,
            Effect::Control,
            Effect::Poll,
            Effect::Network,
            Effect::Control,
            Effect::Drain,
            Effect::Control,
            Effect::Poll,
            Effect::Network,
            Effect::Control,
            Effect::Drain,
            Effect::Control,
            Effect::Poll,
            Effect::Network,
            Effect::Control,
            Effect::Control,
            Effect::Poll,
            Effect::Network,
            Effect::Control
        ]
    );
    assert!(port.finals.is_empty() && port.drains.is_empty());
    let count = port.trace.borrow().len();
    for _ in 0..3 {
        assert!(matches!(
            state.step(&mut port, &mut control),
            Ok(RoomFinalStep::Completed)
        ));
    }
    assert_eq!(port.trace.borrow().len(), count);
}

#[test]
fn terminal_needs_own_admissions_all_receipts_and_healthy_observed_acceptances() {
    for mask in 0..16 {
        for cancelled in [false, true] {
            for failed in [false, true] {
                for final_accepted in [false, true] {
                    for drain_accepted in [false, true] {
                        let (mut port, mut control) = setup(
                            vec![
                                Ok(pending(false, false, false)),
                                Ok(pending(false, true, false)),
                                Ok(terminal(
                                    mask,
                                    cancelled,
                                    failed,
                                    final_accepted,
                                    drain_accepted,
                                )),
                            ],
                            vec![Ok(1), Ok(2), Ok(3)],
                            (10..=14).map(Ok).collect(),
                            vec![Ok(RoomFinalAdmission::Accepted)],
                            vec![Ok(RoomFinalAdmission::Accepted)],
                        );
                        let mut state = RoomFinalWaitState::new(100, 10_000_000);
                        wait(&mut state, &mut port, &mut control, 1_000_000);
                        wait(&mut state, &mut port, &mut control, 1_000_000);
                        let result = state.step(&mut port, &mut control);
                        if mask == 15 && !cancelled && !failed && final_accepted && drain_accepted {
                            assert!(matches!(result, Ok(RoomFinalStep::Completed)));
                        } else {
                            assert!(matches!(result, Err(RoomFinalWaitError::InvalidTerminal)));
                            sealed(&mut state, &mut port, &mut control);
                        }
                        assert!(port.finals.is_empty() && port.drains.is_empty());
                    }
                }
            }
        }
    }
    // An apparently complete remote terminal is insufficient without this
    // owner's actual command admissions; it must not queue after observing it.
    let (mut port, mut control) = setup(
        vec![Ok(terminal(15, false, false, true, true))],
        vec![Ok(1)],
        vec![Ok(1)],
        vec![],
        vec![],
    );
    let mut state = RoomFinalWaitState::new(100, 100);
    assert!(matches!(
        state.step(&mut port, &mut control),
        Err(RoomFinalWaitError::InvalidTerminal)
    ));
    assert_eq!(
        *port.trace.borrow(),
        [Effect::Poll, Effect::Network, Effect::Control]
    );
    sealed(&mut state, &mut port, &mut control);
}

#[test]
fn each_fixed_deadline_precedes_even_complete_terminal_and_never_renews() {
    for network_expired in [false, true] {
        let (mut port, mut control) = setup(
            vec![
                Ok(pending(false, false, false)),
                Ok(pending(false, true, false)),
                Ok(terminal(15, false, false, true, true)),
            ],
            vec![Ok(1), Ok(2), Ok(if network_expired { 100 } else { 3 })],
            vec![
                Ok(1),
                Ok(2),
                Ok(3),
                Ok(4),
                Ok(if network_expired { 5 } else { 100 }),
            ],
            vec![Ok(RoomFinalAdmission::Accepted)],
            vec![Ok(RoomFinalAdmission::Accepted)],
        );
        let mut state = RoomFinalWaitState::new(100, 100);
        wait(&mut state, &mut port, &mut control, 98);
        wait(&mut state, &mut port, &mut control, 96);
        assert!(matches!(
            state.step(&mut port, &mut control),
            Err(RoomFinalWaitError::TimedOut)
        ));
        sealed(&mut state, &mut port, &mut control);
    }
    let (mut port, mut control) = setup(
        vec![
            Ok(pending(false, false, false)),
            Ok(pending(false, false, false)),
        ],
        vec![Ok(9), Ok(10)],
        vec![Ok(1), Ok(2), Ok(3)],
        vec![Ok(RoomFinalAdmission::QueueFull)],
        vec![],
    );
    let mut state = RoomFinalWaitState::new(10, 100);
    wait(&mut state, &mut port, &mut control, 98);
    assert!(matches!(
        state.step(&mut port, &mut control),
        Err(RoomFinalWaitError::TimedOut)
    ));
    assert_eq!(
        port.trace
            .borrow()
            .iter()
            .filter(|event| **event == Effect::Final)
            .count(),
        1
    );
}

#[test]
fn clocks_preserve_cross_step_fresh_observations_and_seal_negative_or_regressed_time() {
    for kind in 0..4 {
        let (mut port, mut control) = setup(
            vec![
                Ok(pending(true, false, false)),
                Ok(pending(true, false, false)),
            ],
            vec![
                Ok(10),
                Ok(if kind == 0 {
                    -1
                } else if kind == 1 {
                    9
                } else {
                    11
                }),
            ],
            vec![Ok(20), Ok(30), Ok(if kind == 2 { 29 } else { 31 }), Ok(30)],
            vec![],
            vec![],
        );
        let mut state = RoomFinalWaitState::new(100, 100);
        wait(&mut state, &mut port, &mut control, 70);
        let result = state.step(&mut port, &mut control);
        if kind == 0 {
            assert!(matches!(result, Err(RoomFinalWaitError::InvalidClock)));
        } else {
            assert!(matches!(result, Err(RoomFinalWaitError::ClockRegressed)));
        }
        sealed(&mut state, &mut port, &mut control);
    }
}

#[test]
fn opaque_errors_preserve_identity_once_and_seal_after_prior_admitted_effects() {
    for stage in 0..6 {
        let marker = Arc::new(stage);
        let (mut port, mut control) = setup(
            vec![
                Ok(pending(false, false, false)),
                Ok(pending(false, true, false)),
            ],
            vec![Ok(1), Ok(2)],
            vec![Ok(10), Ok(11), Ok(12), Ok(13)],
            vec![Ok(RoomFinalAdmission::Accepted)],
            vec![Ok(RoomFinalAdmission::Accepted)],
        );
        let mut state = RoomFinalWaitState::new(100, 100);
        // Establish one actual final admission before injecting the next fault.
        wait(&mut state, &mut port, &mut control, 89);
        match stage {
            0 => port.polls[0] = Err(PortError(marker.clone())),
            1 => port.network[0] = Err(PortError(marker.clone())),
            2 => control.times[0] = Err(ControlError(marker.clone())),
            3 => port.drains[0] = Err(PortError(marker.clone())),
            4 => control.times[1] = Err(ControlError(marker.clone())),
            _ => {
                // A separate first-step final admission refusal also seals.
                state = RoomFinalWaitState::new(100, 100);
                port.finals.push_back(Err(PortError(marker.clone())));
            }
        }
        match state.step(&mut port, &mut control) {
            Err(RoomFinalWaitError::Port(error)) if stage != 2 && stage != 4 => {
                assert!(Arc::ptr_eq(&error.0, &marker));
            }
            Err(RoomFinalWaitError::Control(error)) if stage == 2 || stage == 4 => {
                assert!(Arc::ptr_eq(&error.0, &marker));
            }
            _ => panic!("opaque failure changed identity or variant"),
        }
        sealed(&mut state, &mut port, &mut control);
        assert_eq!(
            port.trace
                .borrow()
                .iter()
                .filter(|event| **event == Effect::Final)
                .count(),
            if stage == 5 { 2 } else { 1 }
        );
        if stage == 4 {
            assert!(
                port.drains.is_empty(),
                "accepted drain survives post-control refusal"
            );
        }
    }
}

#[test]
fn post_command_overshoot_returns_zero_then_fixed_timeout_and_full_width_waits_do_not_overflow() {
    let (mut port, mut control) = setup(
        vec![
            Ok(pending(false, false, false)),
            Ok(terminal(15, false, false, true, true)),
        ],
        vec![Ok(1), Ok(2)],
        vec![Ok(99), Ok(101), Ok(101)],
        vec![Ok(RoomFinalAdmission::Accepted)],
        vec![],
    );
    let mut state = RoomFinalWaitState::new(100, 100);
    wait(&mut state, &mut port, &mut control, 0);
    assert!(matches!(
        state.step(&mut port, &mut control),
        Err(RoomFinalWaitError::TimedOut)
    ));
    sealed(&mut state, &mut port, &mut control);
    for (network, now, deadline, expected) in [
        (
            604_800_000_000_000,
            9_007_199_254_740_993,
            9_007_199_254_740_993 + 1_000_001,
            1_000_000,
        ),
        (i64::MAX - 1, u64::MAX - 1, u64::MAX, 1),
    ] {
        let (mut port, mut control) = setup(
            vec![Ok(pending(true, false, false))],
            vec![Ok(network)],
            vec![Ok(now), Ok(now)],
            vec![],
            vec![],
        );
        let mut state = RoomFinalWaitState::new(i64::MAX, deadline);
        wait(&mut state, &mut port, &mut control, expected);
        assert_eq!(
            *port.trace.borrow(),
            [
                Effect::Poll,
                Effect::Network,
                Effect::Control,
                Effect::Control
            ]
        );
    }
}

#[test]
fn blocking_wrapper_alone_parks_and_park_refusal_stops_without_more_port_effects() {
    for refuse in [false, true] {
        let marker = Arc::new(91);
        let (mut port, mut control) = setup(
            vec![
                Ok(pending(false, false, false)),
                Ok(pending(false, true, false)),
                Ok(terminal(15, false, false, true, true)),
            ],
            vec![Ok(1), Ok(2), Ok(3)],
            (10..=14).map(Ok).collect(),
            vec![Ok(RoomFinalAdmission::Accepted)],
            vec![Ok(RoomFinalAdmission::Accepted)],
        );
        control.allow_park = true;
        if refuse {
            control.park_error = Some(ControlError(marker.clone()));
        }
        let result = wait_for_room_final(&mut port, &mut control, 100, 10_000_000);
        if refuse {
            match result {
                Err(RoomFinalWaitError::Control(error)) => assert!(Arc::ptr_eq(&error.0, &marker)),
                _ => panic!("park refusal changed"),
            }
            assert_eq!(
                *port.trace.borrow(),
                [
                    Effect::Poll,
                    Effect::Network,
                    Effect::Control,
                    Effect::Final,
                    Effect::Control,
                    Effect::Park(1_000_000)
                ]
            );
            assert_eq!(port.polls.len(), 2);
        } else {
            assert!(result.is_ok());
            assert_eq!(
                *port.trace.borrow(),
                [
                    Effect::Poll,
                    Effect::Network,
                    Effect::Control,
                    Effect::Final,
                    Effect::Control,
                    Effect::Park(1_000_000),
                    Effect::Poll,
                    Effect::Network,
                    Effect::Control,
                    Effect::Drain,
                    Effect::Control,
                    Effect::Park(1_000_000),
                    Effect::Poll,
                    Effect::Network,
                    Effect::Control
                ]
            );
        }
    }
}
