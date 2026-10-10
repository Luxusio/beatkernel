//! Portable collector contracts; scripted sources do not establish hardware behavior.
use crate::native_input::{
    CollectorConfig, CollectorError, InputPublisher, NativeInputCollector, NativeInputSource,
    SourceDrain,
};
use beatkernel::{
    input::{
        BackendId, ButtonEvent, ButtonState, CustomInputEvent, DeviceId, EventMeta,
        NativeEventMeta, PhysicalControlId, PhysicalInputEvent, VendorNamespaceId,
    },
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use std::{
    collections::VecDeque,
    rc::Rc,
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, ThreadId},
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(5);

fn point(nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(10),
        timestamp: Timestamp::from_nanos(nanos),
    }
}

fn button(sequence: u64) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(7), point(sequence as i64), sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(3),
        code: Some(30),
        timestamp: Some(ClockPoint {
            domain: ClockDomainId(91),
            timestamp: Timestamp::from_nanos(sequence as i64 - 1),
        }),
    });
    meta.original_clock_point = meta.native.and_then(|native| native.timestamp);
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    })
}

fn payload(capacity: usize) -> PhysicalInputEvent {
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(&[1, 2, 3]);
    PhysicalInputEvent::Custom(CustomInputEvent {
        meta: *button(1).meta(),
        namespace: VendorNamespaceId(9),
        type_id: 17,
        payload: bytes,
    })
}

#[derive(Debug)]
enum OwnerObservation {
    Constructed(ThreadId),
    Polled(ThreadId),
    Closed(ThreadId),
    Dropped(ThreadId),
}

// The Rc deliberately prevents sending the source across threads: only its
// factory configuration may move to the acquisition worker.
struct OwnerWitness {
    observations: Sender<OwnerObservation>,
    _affine: Rc<()>,
}

impl OwnerWitness {
    fn new(observations: Sender<OwnerObservation>) -> Self {
        observations
            .send(OwnerObservation::Constructed(thread::current().id()))
            .unwrap();
        Self {
            observations,
            _affine: Rc::new(()),
        }
    }
}

impl Drop for OwnerWitness {
    fn drop(&mut self) {
        let _ = self
            .observations
            .send(OwnerObservation::Dropped(thread::current().id()));
    }
}

fn receive<T>(rx: &Receiver<T>) -> T {
    rx.recv_timeout(WAIT)
        .expect("collector did not reach the deterministic fixture gate")
}

enum Action {
    Batch(Vec<PhysicalInputEvent>, Option<ClockPoint>),
    Fail,
    Panic,
    End,
    Fence(Sender<()>),
}

struct ScriptSource {
    witness: OwnerWitness,
    actions: Receiver<Action>,
    close_error: bool,
}

impl NativeInputSource for ScriptSource {
    fn service(&mut self, sink: &mut InputPublisher<'_>, _: usize) -> Result<SourceDrain, String> {
        match self.actions.try_recv() {
            Ok(Action::Batch(events, cut)) => {
                let _ = self
                    .witness
                    .observations
                    .send(OwnerObservation::Polled(thread::current().id()));
                for event in events {
                    sink.publish(event).map_err(|error| error.to_string())?;
                }
                Ok(SourceDrain {
                    completed_through: cut,
                    idle: false,
                    closed: false,
                })
            }
            Ok(Action::Fail) => Err("native loss".into()),
            Ok(Action::Panic) => panic!("scripted native panic"),
            Ok(Action::End) => Ok(SourceDrain {
                completed_through: None,
                idle: false,
                closed: true,
            }),
            Ok(Action::Fence(done)) => {
                done.send(()).unwrap();
                Ok(SourceDrain {
                    completed_through: None,
                    idle: true,
                    closed: false,
                })
            }
            Err(_) => Ok(SourceDrain {
                completed_through: None,
                idle: true,
                closed: false,
            }),
        }
    }
    fn close(&mut self) -> Result<(), String> {
        let _ = self
            .witness
            .observations
            .send(OwnerObservation::Closed(thread::current().id()));
        if self.close_error {
            Err("close failed".into())
        } else {
            Ok(())
        }
    }
}

struct DropPanicSource {
    inner: ScriptSource,
    close_panic: bool,
}

impl NativeInputSource for DropPanicSource {
    fn service(
        &mut self,
        sink: &mut InputPublisher<'_>,
        quantum: usize,
    ) -> Result<SourceDrain, String> {
        self.inner.service(sink, quantum)
    }

    fn close(&mut self) -> Result<(), String> {
        let result = self.inner.close();
        if self.close_panic {
            panic!("scripted close panic");
        }
        result
    }
}

impl Drop for DropPanicSource {
    fn drop(&mut self) {
        panic!("scripted source destructor panic");
    }
}

#[test]
fn destructor_panic_preserves_first_cleanup_failure_and_original_source_failure() {
    // close error + drop panic; close panic + drop panic; healthy close + drop
    // panic; native error + close error + drop panic. Each source is constructed
    // on its actual acquisition worker, and all cleanup attempts finish before
    // the terminal outcome is inspected.
    for case in 0..4 {
        let (actions_tx, actions) = mpsc::channel();
        let (observations, observed) = mpsc::channel();
        let mut collector = NativeInputCollector::spawn(config(), move || {
            Ok(DropPanicSource {
                inner: ScriptSource {
                    witness: OwnerWitness::new(observations),
                    actions,
                    close_error: case == 0 || case == 3,
                },
                close_panic: case == 1,
            })
        })
        .unwrap();
        collector.wait_ready().unwrap();
        let owner = match receive(&observed) {
            OwnerObservation::Constructed(id) => id,
            other => panic!("{other:?}"),
        };
        assert_ne!(owner, thread::current().id());
        actions_tx
            .send(if case == 3 { Action::Fail } else { Action::End })
            .unwrap();
        collector.wake();
        assert!(matches!(receive(&observed), OwnerObservation::Closed(id) if id == owner));
        assert!(matches!(receive(&observed), OwnerObservation::Dropped(id) if id == owner));
        let expected = match case {
            0 => CollectorError::Close("close failed".into()),
            1 => CollectorError::Close("source close panicked".into()),
            2 => CollectorError::Close("source drop panicked".into()),
            _ => CollectorError::FailedClose {
                primary: Box::new(CollectorError::Source("native loss".into())),
                close: "close failed".into(),
            },
        };
        assert_eq!(
            collector.stop_and_join(),
            Err(expected.clone()),
            "case {case}"
        );
        assert_eq!(collector.status(), Err(expected), "case {case}");
    }
}

fn config() -> CollectorConfig {
    CollectorConfig {
        domain: ClockDomainId(10),
        entries: 16,
        bytes: 65_536,
        max_payload_bytes: 1024,
        service_quantum: 8,
        idle_wait: Duration::from_secs(1),
    }
}

fn spawn(
    config: CollectorConfig,
    close_error: bool,
) -> (
    NativeInputCollector,
    Sender<Action>,
    Receiver<OwnerObservation>,
) {
    let (actions_tx, actions) = mpsc::channel();
    let (observations, observed) = mpsc::channel();
    let mut collector = NativeInputCollector::spawn(config, move || {
        Ok(ScriptSource {
            witness: OwnerWitness::new(observations),
            actions,
            close_error,
        })
    })
    .unwrap();
    collector.wait_ready().unwrap();
    (collector, actions_tx, observed)
}

// A second service command proves the preceding service has returned and its
// covered events/cut have been processed. No consumer draining occurs here.
fn publish(
    collector: &NativeInputCollector,
    tx: &Sender<Action>,
    events: Vec<PhysicalInputEvent>,
    cut: Option<ClockPoint>,
) {
    tx.send(Action::Batch(events, cut)).unwrap();
    let (done, finished) = mpsc::channel();
    tx.send(Action::Fence(done)).unwrap();
    collector.wake();
    receive(&finished);
}

fn terminal(collector: &mut NativeInputCollector) -> CollectorError {
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        match collector.status() {
            Err(error) => return error,
            Ok(true) => panic!("failure was reported as successful termination"),
            Ok(false) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "fatal status remained hidden"
                );
                thread::yield_now();
            }
        }
    }
}

#[test]
fn affine_source_acquires_while_consumer_is_stalled_and_closes_on_owner() {
    let caller = thread::current().id();
    let (mut collector, tx, observed) = spawn(config(), false);
    let owner = match receive(&observed) {
        OwnerObservation::Constructed(id) => id,
        other => panic!("{other:?}"),
    };
    assert_ne!(owner, caller);
    let expected = vec![button(1), button(2), payload(64)];
    publish(&collector, &tx, expected.clone(), Some(point(3)));
    assert!(matches!(receive(&observed), OwnerObservation::Polled(id) if id == owner));
    let mut out = VecDeque::new();
    let first = collector.drain(&mut out, 1).unwrap();
    assert_eq!(out, VecDeque::from([expected[0].clone()]));
    assert_eq!(first.items, 1);
    assert!(first.backlog);
    assert_eq!(first.completed_through, None);
    let second = collector.drain(&mut out, 2).unwrap();
    assert_eq!(out, VecDeque::from(expected));
    assert_eq!(second.completed_through, None);
    let barrier = collector.drain(&mut out, 1).unwrap();
    assert_eq!(barrier.completed_through, Some(point(3)));
    assert_eq!(
        collector.drain(&mut out, 1).unwrap().completed_through,
        None
    );
    collector.stop_and_join().unwrap();
    assert!(matches!(receive(&observed), OwnerObservation::Closed(id) if id == owner));
    assert!(matches!(receive(&observed), OwnerObservation::Dropped(id) if id == owner));
}

#[test]
fn unavailable_cut_and_later_receipt_clock_never_invent_native_completion() {
    let (mut collector, tx, _) = spawn(config(), false);
    publish(&collector, &tx, vec![button(1)], None);
    let receipt_clock = point(10_000);
    let mut out = VecDeque::new();
    assert_eq!(
        collector.drain(&mut out, 8).unwrap().completed_through,
        None
    );
    assert_eq!(
        collector.drain(&mut out, 8).unwrap().completed_through,
        None
    );
    assert_eq!(out.front().unwrap().meta().timestamp, point(1).timestamp);
    assert_ne!(receipt_clock, point(1));
    collector.stop_and_join().unwrap();
}

#[test]
fn repeated_completed_cut_does_not_overflow_a_full_marker_queue() {
    let mut limits = config();
    limits.entries = 1;
    let (mut collector, tx, _) = spawn(limits, false);
    publish(&collector, &tx, vec![], Some(point(10)));
    publish(&collector, &tx, vec![], Some(point(10)));
    assert_eq!(collector.status(), Ok(false));
    let mut out = VecDeque::new();
    assert_eq!(
        collector.drain(&mut out, 1).unwrap().completed_through,
        Some(point(10))
    );
    assert_eq!(
        collector.drain(&mut out, 1).unwrap().completed_through,
        None
    );
    assert!(out.is_empty());
    collector.stop_and_join().unwrap();
}

#[test]
fn invalid_domain_and_regressing_cut_are_explicit() {
    for case in 0..3 {
        let (mut collector, tx, _) = spawn(config(), false);
        publish(&collector, &tx, vec![], Some(point(10)));
        let mut wrong_domain = point(11);
        wrong_domain.domain = ClockDomainId(99);
        let (events, cut) = match case {
            0 => (vec![], Some(wrong_domain)),
            1 => (vec![], Some(point(9))),
            _ => {
                let mut event = button(11);
                event.meta_mut().clock_domain = ClockDomainId(99);
                (vec![event], None)
            }
        };
        tx.send(Action::Batch(events, cut)).unwrap();
        collector.wake();
        let error = terminal(&mut collector);
        assert!(
            match case {
                0 | 2 => matches!(error, CollectorError::DomainMismatch),
                1 => matches!(error, CollectorError::CutRegression),
                _ => unreachable!(),
            },
            "case {case}: {error:?}"
        );
        assert!(collector.stop_and_join().is_err());
    }
}

#[test]
fn delayed_native_timestamp_behind_raw_cut_is_transferred_without_retiming() {
    let (mut collector, tx, _) = spawn(config(), false);
    publish(&collector, &tx, vec![], Some(point(10)));
    let mut out = VecDeque::new();
    assert_eq!(
        collector.drain(&mut out, 1).unwrap().completed_through,
        Some(point(10))
    );
    let original = button(9);
    publish(&collector, &tx, vec![original.clone()], None);
    assert_eq!(
        collector.drain(&mut out, 1).unwrap().completed_through,
        None
    );
    assert_eq!(out, VecDeque::from([original]));
    collector.stop_and_join().unwrap();
}

#[test]
fn capacity_failures_are_fatal_even_without_consumer_space() {
    for case in 0..4 {
        let mut limits = config();
        let events = match case {
            0 => {
                limits.entries = 1;
                vec![button(1), button(2)]
            }
            1 => {
                limits.bytes = 512;
                limits.max_payload_bytes = 512;
                vec![payload(512)]
            }
            2 => {
                limits.max_payload_bytes = 32;
                vec![payload(64)]
            }
            _ => {
                limits.service_quantum = 1;
                vec![button(1), button(2)]
            }
        };
        let (mut collector, tx, _) = spawn(limits, false);
        tx.send(Action::Batch(events, None)).unwrap();
        collector.wake();
        let error = terminal(&mut collector);
        assert!(
            match case {
                0 => matches!(error, CollectorError::EntryCapacity),
                1 => matches!(error, CollectorError::ByteCapacity),
                2 => matches!(error, CollectorError::PayloadCapacity),
                _ => matches!(error, CollectorError::ServiceQuantum),
            },
            "case {case}: {error:?}"
        );
        assert!(collector.drain(&mut VecDeque::new(), 8).is_err());
        assert!(collector.stop_and_join().is_err());
    }
}

#[test]
fn cancellation_wakes_idle_and_full_workers_without_draining() {
    for full in [false, true] {
        let mut limits = config();
        limits.entries = 1;
        let (mut collector, tx, observed) = spawn(limits, false);
        let _ = receive(&observed);
        if full {
            publish(&collector, &tx, vec![button(1)], None);
            let _ = receive(&observed);
        }
        collector.cancel();
        collector.stop_and_join().unwrap();
        assert!(matches!(receive(&observed), OwnerObservation::Closed(_)));
        assert!(matches!(receive(&observed), OwnerObservation::Dropped(_)));
    }
}

#[test]
fn source_failure_panic_and_cleanup_failure_join_and_preserve_first_cause() {
    for case in 0..3 {
        let (mut collector, tx, observed) = spawn(config(), case != 1);
        let _ = receive(&observed);
        if case < 2 {
            tx.send(if case == 0 {
                Action::Fail
            } else {
                Action::Panic
            })
            .unwrap();
            collector.wake();
            let error = terminal(&mut collector);
            assert!(match case {
                0 =>
                    matches!(error, CollectorError::Source(_))
                        || matches!(error, CollectorError::FailedClose { primary, .. } if matches!(*primary, CollectorError::Source(_))),
                _ => matches!(error, CollectorError::Panic),
            });
        }
        let error = collector.stop_and_join().unwrap_err();
        assert!(match case {
            0 =>
                matches!(error, CollectorError::FailedClose { primary, .. } if matches!(*primary, CollectorError::Source(_))),
            1 => matches!(error, CollectorError::Panic),
            _ => matches!(error, CollectorError::Close(_)),
        });
        assert!(matches!(receive(&observed), OwnerObservation::Closed(_)));
        assert!(matches!(receive(&observed), OwnerObservation::Dropped(_)));
    }
}

#[test]
fn normal_native_end_drains_original_events_and_is_distinct_from_failure() {
    let (mut collector, tx, observed) = spawn(config(), false);
    let _ = receive(&observed);
    publish(&collector, &tx, vec![button(1)], Some(point(2)));
    let _ = receive(&observed);
    tx.send(Action::End).unwrap();
    collector.wake();
    assert!(matches!(receive(&observed), OwnerObservation::Closed(_)));
    assert!(matches!(receive(&observed), OwnerObservation::Dropped(_)));
    collector.stop_and_join().unwrap();
    assert_eq!(collector.status(), Ok(true));
    let mut out = VecDeque::new();
    let no_budget = collector.drain(&mut out, 0).unwrap();
    assert_eq!(no_budget.items, 0);
    assert_eq!(no_budget.completed_through, None);
    assert!(no_budget.backlog);
    assert!(!no_budget.closed, "queued terminal data is not exhaustion");
    assert!(out.is_empty());
    let data = collector.drain(&mut out, 1).unwrap();
    assert_eq!(out, VecDeque::from([button(1)]));
    assert_eq!(data.items, 1);
    assert!(data.backlog);
    assert!(!data.closed, "the final original event must reach gameplay");
    assert_eq!(data.completed_through, None);
    let cut = collector.drain(&mut out, 1).unwrap();
    assert_eq!(cut.items, 1);
    assert!(!cut.backlog);
    assert!(!cut.closed, "the final original cut must reach gameplay");
    assert_eq!(cut.completed_through, Some(point(2)));
    for _ in 0..2 {
        let exhausted = collector.drain(&mut out, 1).unwrap();
        assert_eq!(exhausted.items, 0);
        assert!(!exhausted.backlog);
        assert!(exhausted.closed);
        assert_eq!(exhausted.completed_through, None);
        assert_eq!(out, VecDeque::from([button(1)]));
    }
}

#[test]
fn factory_failure_is_visible_and_joinable_without_constructing_source() {
    let mut collector =
        NativeInputCollector::spawn(config(), || -> Result<ScriptSource, String> {
            Err("open failed".into())
        })
        .unwrap();
    assert!(matches!(
        collector.wait_ready(),
        Err(CollectorError::Initialization(_))
    ));
    assert!(matches!(
        collector.stop_and_join(),
        Err(CollectorError::Initialization(_))
    ));
}

#[test]
fn factory_panic_is_visible_and_joinable() {
    let mut collector =
        NativeInputCollector::spawn(config(), || -> Result<ScriptSource, String> {
            panic!("factory panicked before source construction")
        })
        .unwrap();
    assert!(matches!(collector.wait_ready(), Err(CollectorError::Panic)));
    assert!(matches!(
        collector.stop_and_join(),
        Err(CollectorError::Panic)
    ));
}

#[test]
fn zero_limits_reject_before_factory_side_effects() {
    for case in 0..3 {
        let mut limits = config();
        match case {
            0 => limits.entries = 0,
            1 => limits.bytes = 0,
            _ => limits.service_quantum = 0,
        }
        let result = NativeInputCollector::spawn(limits, || -> Result<ScriptSource, String> {
            panic!("invalid limits must not construct a source")
        });
        assert!(matches!(result, Err(CollectorError::InvalidConfiguration)));
    }
}

#[test]
fn dropping_collector_cancels_and_joins_affine_source() {
    let (collector, _, observed) = spawn(config(), false);
    let owner = match receive(&observed) {
        OwnerObservation::Constructed(id) => id,
        other => panic!("{other:?}"),
    };
    drop(collector);
    assert!(matches!(receive(&observed), OwnerObservation::Closed(id) if id == owner));
    assert!(matches!(receive(&observed), OwnerObservation::Dropped(id) if id == owner));
}
