//! Exercises the production adapter seam; mocked pumps do not prove native runloop cadence.
use super::*;
use beatkernel::{
    input::{BackendId, ButtonEvent, ButtonState, EventMeta, NativeEventMeta, PhysicalControlId},
    time::{ClockDomainId, Timestamp},
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

fn point(nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(2),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn event(source: u64, sequence: u64) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(source), point(7), sequence);
    let original = ClockPoint {
        domain: ClockDomainId(1),
        timestamp: Timestamp::from_nanos(700),
    };
    meta.original_clock_point = Some(original);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(0x4d48_4944),
        code: Some(4),
        timestamp: Some(original),
    });
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    })
}
struct Step {
    pump: Result<Pump, String>,
    events: Vec<PhysicalInputEvent>,
    health: Option<String>,
}
struct Script {
    pending: VecDeque<PhysicalInputEvent>,
    steps: VecDeque<Step>,
    current: Cell<i64>,
    health_error: Option<String>,
    operations: RefCell<Vec<&'static str>>,
    // A real native manager is affine too: this scripted source cannot be Send.
    _affine: Rc<()>,
}
impl Acquisition for Script {
    fn poll(&mut self) -> Result<Pump, String> {
        self.operations.borrow_mut().push("poll");
        let step = self
            .steps
            .pop_front()
            .expect("pump exceeded declared finite fixture budget");
        self.current.set(self.current.get() + 100);
        self.pending.extend(step.events);
        self.health_error = step.health;
        step.pump
    }
    fn pop(&mut self) -> Option<PhysicalInputEvent> {
        self.operations.borrow_mut().push("pop");
        self.pending.pop_front()
    }
    fn health(&self) -> Result<(), String> {
        self.operations.borrow_mut().push("health");
        self.health_error.clone().map_or(Ok(()), Err)
    }
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}
fn step(pump: Pump, events: Vec<PhysicalInputEvent>) -> Step {
    Step {
        pump: Ok(pump),
        events,
        health: None,
    }
}
fn source(events: Vec<PhysicalInputEvent>, steps: Vec<Step>) -> Source<Script> {
    Source {
        acquisition: Script {
            pending: events.into(),
            steps: steps.into(),
            current: Cell::new(20),
            health_error: None,
            operations: RefCell::new(Vec::new()),
            _affine: Rc::new(()),
        },
        selected: vec![DeviceId(1), DeviceId(2)],
        active: Arc::new(AtomicBool::new(true)),
    }
}

#[test]
fn empty_callback_buffer_with_handled_source_withholds_completion() {
    let mut input = source(
        vec![],
        vec![step(Pump::Handled, vec![]), step(Pump::Handled, vec![])],
    );
    let result = input
        .drain(2, |_| panic!("no callback event exists"))
        .unwrap();
    assert_eq!(result.completed_through, None);
    assert!(!result.idle);
    assert_eq!(
        input
            .acquisition
            .operations
            .borrow()
            .iter()
            .filter(|op| **op == "poll")
            .count(),
        2
    );
}

#[test]
fn proven_idle_pump_publishes_all_callbacks_before_original_predrain_cut() {
    let original = vec![event(1, 1), event(2, 2)];
    let mut input = source(vec![], vec![step(Pump::IdleAt(point(20)), original.clone())]);
    let mut actual = Vec::new();
    let result = input
        .drain(4, |value| {
            actual.push(value);
            Ok(())
        })
        .unwrap();
    assert_eq!(actual, original);
    assert_eq!(result.completed_through, Some(point(20)));
    assert!(result.idle);
    assert_eq!(input.acquisition.current.get(), 120);
    assert_eq!(
        &input.acquisition.operations.borrow()[..3],
        &["health", "pop", "poll"]
    );
}

#[test]
fn checked_native_all_queue_cut_is_retained_after_covered_values_are_published() {
    let original = vec![event(1, 1), event(2, 2)];
    // The queue sweep began earlier than this adapter's current observation.
    // Its actual conservative cut must survive unchanged through the adapter.
    let mut input = source(vec![], vec![step(Pump::IdleAt(point(5)), original.clone())]);
    let mut actual = Vec::new();
    let result = input
        .drain(4, |value| {
            actual.push(value);
            Ok(())
        })
        .unwrap();
    assert_eq!(actual, original);
    assert_eq!(result.completed_through, Some(point(5)));
    assert_ne!(result.completed_through, Some(point(20)));
    assert!(result.idle);
    assert_eq!(actual[0].meta().timestamp, point(7).timestamp);
    assert_eq!(
        actual[0]
            .meta()
            .native
            .unwrap()
            .timestamp
            .unwrap()
            .timestamp,
        Timestamp::from_nanos(700)
    );
}

#[test]
fn checked_native_empty_at_budget_end_requires_later_covered_buffer_empty() {
    let mut input = source(
        vec![],
        vec![step(Pump::IdleAt(point(5)), vec![event(1, 1)])],
    );
    let result = input
        .drain(1, |_| panic!("value still belongs to callback buffer"))
        .unwrap();
    assert_eq!(result.completed_through, None);
    assert_eq!(input.acquisition.pending.len(), 1);
}

#[test]
fn exhausting_budget_at_idle_or_last_event_never_exposes_cut() {
    for quantum in [1, 2] {
        let mut input = source(vec![], vec![step(Pump::IdleAt(point(20)), vec![event(1, 1)])]);
        let mut actual = Vec::new();
        let result = input
            .drain(quantum, |value| {
                actual.push(value);
                Ok(())
            })
            .unwrap();
        assert_eq!(result.completed_through, None);
        assert!(!result.idle);
        assert_eq!(actual.len(), quantum - 1);
    }
}

#[test]
fn ignored_device_callbacks_also_consume_service_quantum() {
    let mut input = source(vec![event(99, 1), event(99, 2), event(1, 3)], vec![]);
    let result = input
        .drain(2, |_| panic!("ignored source cannot publish"))
        .unwrap();
    assert_eq!(result.completed_through, None);
    assert_eq!(input.acquisition.pending, VecDeque::from([event(1, 3)]));
    assert!(!input.acquisition.operations.borrow().contains(&"poll"));
}

#[test]
fn loss_or_removal_of_any_selected_source_prevents_idle_cut() {
    for error in [
        "selected device2 removed",
        "callback overflow",
        "invalid native value",
        "timestamp overflow",
    ] {
        let mut failing = step(Pump::IdleAt(point(20)), vec![]);
        failing.health = Some(error.into());
        let mut input = source(vec![], vec![failing]);
        assert_eq!(input.drain(3, |_| Ok(())).unwrap_err(), error);
    }
    let mut input = source(vec![event(1, 1)], vec![]);
    input.acquisition.health_error = Some("selected device2 removed".into());
    assert!(input
        .drain(3, |_| panic!("health must precede publishing"))
        .is_err());
}

#[test]
fn stopped_runloop_poll_failure_and_publication_failure_are_explicit() {
    let mut input = source(vec![], vec![step(Pump::Stopped, vec![])]);
    assert!(input
        .drain(3, |_| Ok(()))
        .unwrap_err()
        .contains("stopped/finished"));
    let mut failure = step(Pump::Handled, vec![]);
    failure.pump = Err("native poll failure".into());
    let mut input = source(vec![], vec![failure]);
    assert_eq!(
        input.drain(3, |_| Ok(())).unwrap_err(),
        "native poll failure"
    );
    let mut input = source(vec![event(1, 1), event(2, 2)], vec![]);
    assert_eq!(
        input
            .drain(3, |_| Err("transport full".into()))
            .unwrap_err(),
        "transport full"
    );
    assert_eq!(input.acquisition.pending, VecDeque::from([event(2, 2)]));
}

#[test]
fn inactive_startup_source_never_samples_or_pumps_before_activation() {
    let mut input = source(vec![event(1, 1)], vec![]);
    input.active.store(false, Ordering::Release);
    let result = input.drain(10, |_| panic!("inactive source")).unwrap();
    assert!(result.idle);
    assert_eq!(result.completed_through, None);
    assert!(input.acquisition.operations.borrow().is_empty());
    assert_eq!(input.acquisition.pending.len(), 1);
}

#[test]
fn affine_adapter_is_constructed_serviced_closed_and_dropped_on_worker_with_full_queue() {
    use beatkernel_bms_runtime::native_input::{
        CollectorConfig, CollectorError, NativeInputCollector,
    };
    use std::{
        sync::mpsc,
        thread::{self, ThreadId},
        time::Duration,
    };
    struct Affine {
        event: Option<PhysicalInputEvent>,
        ready: Option<mpsc::Sender<ThreadId>>,
        lifecycle: mpsc::Sender<(&'static str, ThreadId)>,
        fail_close: bool,
        _affine: Rc<()>,
    }
    impl Acquisition for Affine {
        fn poll(&mut self) -> Result<Pump, String> {
            if let Some(ready) = self.ready.take() {
                ready.send(thread::current().id()).unwrap();
            }
            Ok(Pump::Handled)
        }
        fn pop(&mut self) -> Option<PhysicalInputEvent> {
            self.event.take()
        }
        fn health(&self) -> Result<(), String> {
            Ok(())
        }
        fn close(&mut self) -> Result<(), String> {
            self.lifecycle
                .send(("close", thread::current().id()))
                .unwrap();
            if self.fail_close {
                Err("native close retained".into())
            } else {
                Ok(())
            }
        }
    }
    impl Drop for Affine {
        fn drop(&mut self) {
            let _ = self.lifecycle.send(("drop", thread::current().id()));
        }
    }
    for fail_close in [false, true] {
        let (lifecycle, observed) = mpsc::channel();
        let (ready, published) = mpsc::channel();
        let caller = thread::current().id();
        let mut collector = NativeInputCollector::spawn(
            CollectorConfig {
                domain: ClockDomainId(2),
                entries: 1,
                bytes: 4096,
                max_payload_bytes: 64,
                service_quantum: 2,
                idle_wait: Duration::from_secs(1),
            },
            move || {
                lifecycle
                    .send(("construct", thread::current().id()))
                    .unwrap();
                Ok(Source {
                    acquisition: Affine {
                        event: Some(event(1, 1)),
                        ready: Some(ready),
                        lifecycle,
                        fail_close,
                        _affine: Rc::new(()),
                    },
                    selected: vec![DeviceId(1)],
                    active: Arc::new(AtomicBool::new(true)),
                })
            },
        )
        .unwrap();
        collector.wait_ready().unwrap();
        let (kind, owner) = observed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(kind, "construct");
        assert_ne!(owner, caller);
        // The production adapter publishes its event before pumping the runloop.
        // This gate therefore proves the one-entry transport is full; do not drain.
        assert_eq!(
            published.recv_timeout(Duration::from_secs(5)).unwrap(),
            owner
        );
        collector.cancel();
        let result = collector.stop_and_join();
        if fail_close {
            assert_eq!(
                result,
                Err(CollectorError::Close("native close retained".into()))
            );
        } else {
            assert_eq!(result, Ok(()));
        }
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(5)).unwrap(),
            ("close", owner)
        );
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(5)).unwrap(),
            ("drop", owner)
        );
    }
}
