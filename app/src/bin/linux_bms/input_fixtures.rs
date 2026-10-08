//! Portable fixtures call the same evdev sweep used by native devices. No
//! scripted descriptor or clock establishes physical Linux device behavior.
use super::{DrainClock, EvdevDrain, EvdevRead};
use beatkernel::{
    input::{
        BackendId, ButtonEvent, ButtonState, DeviceId, EventMeta, NativeEventMeta,
        PhysicalControlId, PhysicalInputEvent,
    },
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use beatkernel_bms_runtime::native_input::{
    CollectorConfig, InputPublisher, NativeInputCollector, NativeInputSource, SourceDrain,
};
use beatkernel_platform::linux::{EvdevItem, EvdevSnapshot};
use std::{
    collections::VecDeque,
    rc::Rc,
    sync::{mpsc, Arc, Mutex},
    thread::{self, ThreadId},
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(5);
fn point(n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(10),
        timestamp: Timestamp::from_nanos(n),
    }
}
fn event(source: u64, sequence: u64) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(source), point(30 + sequence as i64), sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(3),
        code: Some(30),
        timestamp: Some(point(29 + sequence as i64)),
    });
    meta.original_clock_point = meta.native.and_then(|native| native.timestamp);
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    })
}

struct Device {
    id: usize,
    script: VecDeque<Result<EvdevItem, String>>,
    reads: Arc<Mutex<Vec<usize>>>,
    drops: mpsc::Sender<ThreadId>,
    _affine: Rc<()>,
}
impl EvdevRead for Device {
    fn read_next(&mut self) -> Result<EvdevItem, String> {
        self.reads.lock().unwrap().push(self.id);
        self.script.pop_front().unwrap_or(Ok(EvdevItem::WouldBlock))
    }
}
impl Drop for Device {
    fn drop(&mut self) {
        let _ = self.drops.send(thread::current().id());
    }
}
struct Clock {
    samples: VecDeque<ClockPoint>,
}
impl DrainClock for Clock {
    fn now(&mut self) -> Result<ClockPoint, String> {
        self.samples
            .pop_front()
            .ok_or_else(|| "fixture clock exhausted".into())
    }
}

enum Observation {
    Waiting,
    Done(SourceDrain),
    Failed,
}
struct Gated {
    drain: EvdevDrain<Device, Clock>,
    permit: mpsc::Receiver<bool>,
    observe: mpsc::Sender<Observation>,
}
impl NativeInputSource for Gated {
    fn service(
        &mut self,
        sink: &mut InputPublisher<'_>,
        quantum: usize,
    ) -> Result<SourceDrain, String> {
        self.observe.send(Observation::Waiting).unwrap();
        if !self
            .permit
            .recv_timeout(WAIT)
            .map_err(|_| "fixture permit expired")?
        {
            return Ok(SourceDrain {
                idle: true,
                ..SourceDrain::default()
            });
        }
        match self.drain.service(sink, quantum) {
            Ok(result) => {
                self.observe.send(Observation::Done(result)).unwrap();
                Ok(result)
            }
            Err(error) => {
                self.observe.send(Observation::Failed).unwrap();
                Err(error)
            }
        }
    }
    fn close(&mut self) -> Result<(), String> {
        self.drain.close()
    }
}
struct Harness {
    collector: NativeInputCollector,
    permit: mpsc::Sender<bool>,
    observe: mpsc::Receiver<Observation>,
    owner: ThreadId,
    drops: mpsc::Receiver<ThreadId>,
    reads: Arc<Mutex<Vec<usize>>>,
    devices: usize,
}
impl Harness {
    fn new(
        scripts: Vec<Vec<Result<EvdevItem, String>>>,
        samples: &[i64],
        quantum: usize,
        entries: usize,
    ) -> Self {
        let devices = scripts.len();
        let samples = samples.iter().copied().map(point).collect();
        let reads = Arc::new(Mutex::new(Vec::new()));
        let worker_reads = reads.clone();
        let (permit, commands) = mpsc::channel();
        let (observations, observe) = mpsc::channel();
        let (opened, owner) = mpsc::channel();
        let (drop_sender, drops) = mpsc::channel();
        let mut collector = NativeInputCollector::spawn(
            CollectorConfig {
                domain: point(0).domain,
                entries,
                bytes: 1 << 20,
                max_payload_bytes: 4096,
                service_quantum: quantum,
                idle_wait: Duration::from_millis(1),
            },
            move || {
                opened.send(thread::current().id()).unwrap();
                let devices = scripts
                    .into_iter()
                    .enumerate()
                    .map(|(id, script)| Device {
                        id,
                        script: script.into(),
                        reads: worker_reads.clone(),
                        drops: drop_sender.clone(),
                        _affine: Rc::new(()),
                    })
                    .collect();
                Ok(Gated {
                    drain: EvdevDrain::new(devices, Clock { samples })?,
                    permit: commands,
                    observe: observations,
                })
            },
        )
        .unwrap();
        collector.wait_ready().unwrap();
        let owner = owner.recv_timeout(WAIT).unwrap();
        assert_ne!(owner, thread::current().id());
        assert!(matches!(
            observe.recv_timeout(WAIT).unwrap(),
            Observation::Waiting
        ));
        Self {
            collector,
            permit,
            observe,
            owner,
            drops,
            reads,
            devices,
        }
    }
    fn step(&mut self) -> SourceDrain {
        self.permit.send(true).unwrap();
        let Observation::Done(done) = self.observe.recv_timeout(WAIT).unwrap() else {
            panic!("native sweep unexpectedly failed");
        };
        // The next service gate is reached after the collector publishes the
        // preceding completed marker; no timing sleep proves FIFO publication.
        assert!(matches!(
            self.observe.recv_timeout(WAIT).unwrap(),
            Observation::Waiting
        ));
        done
    }
    fn take(&mut self, limit: usize) -> (VecDeque<PhysicalInputEvent>, Option<ClockPoint>) {
        let mut events = VecDeque::new();
        let batch = self.collector.drain(&mut events, limit).unwrap();
        (events, batch.completed_through)
    }
    fn finish(mut self) {
        self.collector.cancel();
        self.permit.send(false).unwrap();
        self.collector.stop_and_join().unwrap();
        for _ in 0..self.devices {
            assert_eq!(self.drops.recv_timeout(WAIT).unwrap(), self.owner);
        }
    }
    fn fatal(mut self) {
        self.permit.send(true).unwrap();
        assert!(matches!(
            self.observe.recv_timeout(WAIT).unwrap(),
            Observation::Failed
        ));
        assert!(self.collector.stop_and_join().is_err());
        for _ in 0..self.devices {
            assert_eq!(self.drops.recv_timeout(WAIT).unwrap(), self.owner);
        }
    }
}

#[test]
fn fair_budget_counts_ignored_records_and_withholds_until_last_source_would_block() {
    let a = event(7, 1);
    let b = event(9, 2);
    let mut fixture = Harness::new(
        vec![
            vec![
                Ok(EvdevItem::Ignored),
                Ok(EvdevItem::Event(a.clone())),
                Ok(EvdevItem::WouldBlock),
            ],
            vec![Ok(EvdevItem::Event(b.clone())), Ok(EvdevItem::WouldBlock)],
        ],
        &[100, 1000, 2000, 3000],
        2,
        32,
    );
    assert_eq!(fixture.step().completed_through, None);
    let (events, cut) = fixture.take(32);
    assert_eq!(events, VecDeque::from([b.clone()]));
    assert_eq!(cut, None);
    assert_eq!(*fixture.reads.lock().unwrap(), vec![0, 1]);
    assert_eq!(fixture.step().completed_through, None);
    let (events, cut) = fixture.take(32);
    assert_eq!(events, VecDeque::from([a.clone()]));
    assert_eq!(cut, None);
    assert_eq!(*fixture.reads.lock().unwrap(), vec![0, 1, 0, 1]);
    let completed = fixture.step();
    assert_eq!(completed.completed_through, Some(point(100)));
    assert_eq!(fixture.take(32).1, Some(point(100)));
    fixture.finish();
}

#[test]
fn covered_native_metadata_precedes_cut_and_empty_transfer_does_not_reinvent_it() {
    let original = event(73, 981);
    let mut fixture = Harness::new(
        vec![vec![
            Ok(EvdevItem::Event(original.clone())),
            Ok(EvdevItem::WouldBlock),
        ]],
        &[5000, 9000],
        8,
        32,
    );
    assert_eq!(fixture.step().completed_through, Some(point(5000)));
    let (events, cut) = fixture.take(1);
    assert_eq!(events, VecDeque::from([original]));
    assert_eq!(cut, None);
    assert_eq!(fixture.take(1).1, Some(point(5000)));
    assert_eq!(fixture.take(32).1, None);
    fixture.finish();
}

#[test]
fn dropped_resync_and_detach_are_fatal_on_the_actual_sweep() {
    let snapshot = EvdevSnapshot {
        device: DeviceId(7),
        observed_at: point(10),
        pressed_keys: vec![30],
        absolute_axes: Vec::new(),
    };
    for item in [
        Ok(EvdevItem::Dropped),
        Ok(EvdevItem::Resync(snapshot)),
        Err("evdev device detached".into()),
    ] {
        Harness::new(vec![vec![item]], &[100], 8, 32).fatal();
    }
}

#[test]
fn publication_capacity_is_fatal_instead_of_discarding_native_event() {
    Harness::new(
        vec![vec![
            Ok(EvdevItem::Event(event(7, 1))),
            Ok(EvdevItem::Event(event(7, 2))),
        ]],
        &[100],
        8,
        1,
    )
    .fatal();
}

#[test]
fn an_unserviced_selected_source_withholds_common_cut() {
    let mut fixture = Harness::new(
        vec![
            vec![Ok(EvdevItem::WouldBlock)],
            vec![
                Ok(EvdevItem::Ignored),
                Ok(EvdevItem::Ignored),
                Ok(EvdevItem::WouldBlock),
            ],
        ],
        &[100, 200, 300, 400],
        1,
        32,
    );
    for _ in 0..3 {
        assert_eq!(fixture.step().completed_through, None);
        assert_eq!(fixture.take(32).1, None);
    }
    assert_eq!(fixture.step().completed_through, Some(point(100)));
    assert_eq!(fixture.take(32).1, Some(point(100)));
    fixture.finish();
}

#[test]
fn startup_retention_hands_off_same_evdev_owner_and_original_sequence_to_gameplay() {
    use beatkernel_bms_runtime::native_gameplay::NativeCollectedInput;
    let first = event(7, 11);
    let second = event(7, 12);
    let mut fixture = Harness::new(
        vec![vec![
            Ok(EvdevItem::Event(first.clone())),
            Ok(EvdevItem::WouldBlock),
            Ok(EvdevItem::Event(second.clone())),
            Ok(EvdevItem::WouldBlock),
        ]],
        &[100, 200],
        8,
        32,
    );
    let mut input = NativeCollectedInput::new().unwrap();
    let mut discarded = 0;
    fixture.step();
    assert!(input
        .service_start(&mut fixture.collector, true, &mut discarded, 32)
        .unwrap());
    assert_eq!(discarded, 0);
    let mut events = VecDeque::new();
    let batch = input
        .acquire(&mut fixture.collector, &mut events, 32)
        .unwrap();
    assert_eq!(events, VecDeque::from([first]));
    assert_eq!(batch.completed_through, Some(point(100)));
    events.clear();
    fixture.step();
    let batch = input
        .acquire(&mut fixture.collector, &mut events, 32)
        .unwrap();
    assert_eq!(events, VecDeque::from([second]));
    assert_eq!(batch.completed_through, Some(point(200)));
    assert_eq!(fixture.reads.lock().unwrap().len(), 4);
    fixture.finish();
}

#[test]
fn startup_discard_counts_original_event_without_reopening_source() {
    use beatkernel_bms_runtime::native_gameplay::NativeCollectedInput;
    let later = event(7, 2);
    let mut fixture = Harness::new(
        vec![vec![
            Ok(EvdevItem::Event(event(7, 1))),
            Ok(EvdevItem::WouldBlock),
            Ok(EvdevItem::Event(later.clone())),
            Ok(EvdevItem::WouldBlock),
        ]],
        &[100, 200],
        8,
        32,
    );
    let mut input = NativeCollectedInput::new().unwrap();
    let mut discarded = 0;
    fixture.step();
    input
        .service_start(&mut fixture.collector, false, &mut discarded, 32)
        .unwrap();
    assert_eq!(discarded, 1);
    fixture.step();
    let mut events = VecDeque::new();
    let batch = input
        .acquire(&mut fixture.collector, &mut events, 32)
        .unwrap();
    assert_eq!(events, VecDeque::from([later]));
    assert_eq!(batch.completed_through, Some(point(200)));
    fixture.finish();
}

#[test]
fn native_factory_rejects_empty_admitted_selection_without_opening_hardware() {
    assert!(super::open(Vec::new()).is_err());
}
