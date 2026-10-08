// Actual solo pump: the fixture output clock keeps moving while acquisition
// completion stays unavailable or at its independently observed earlier cut.
struct CollectorCutDevice<'a> {
    inner: &'a mut Device,
    cuts: Vec<Option<ClockPoint>>,
    empty: bool,
}
impl NativeGameplayDevice for CollectorCutDevice<'_> {
    fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
        self.inner.observe(discipline)
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        self.inner.render_report()
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        self.inner.host_now()
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        if self.inner.step > 8 {
            return Err("collector-cut fixture exhausted".into());
        }
        if !self.empty && self.inner.step == 2 {
            retain_input(
                events,
                input(
                    if self.inner.invalid {
                        30_000_000
                    } else {
                        20_000_000
                    },
                    1,
                    ButtonState::Down,
                ),
            )?;
        }
        Ok(InputBatch {
            completed_through: self
                .cuts
                .get(self.inner.step as usize - 1)
                .copied()
                .flatten(),
            backlog: false,
            closed: self.inner.step == 8,
        })
    }
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        discipline: &PresentationDiscipline,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        self.inner.observe_end(end, discipline, report)
    }
    fn seed_resume(
        &mut self,
        discipline: &mut PresentationDiscipline,
        reference: ClockPair,
    ) -> NativeGameplayResult<()> {
        self.inner.seed_resume(discipline, reference)
    }
    fn fallback_schedule(&mut self, rate: u32) -> NativeGameplayResult<ClockPoint> {
        self.inner.fallback_schedule(rate)
    }
}
fn run_collector_cut(
    f: &mut Fixture,
    finite: bool,
    empty: bool,
    cuts: Vec<Option<ClockPoint>>,
) -> NativeGameplayResult<()> {
    run_gameplay(
        &mut CollectorCutDevice {
            inner: &mut f.device,
            cuts,
            empty,
        },
        NativeGameplaySession {
            runtime: &mut f.runtime,
            gauge: &mut f.gauge,
            bgm: &mut f.bgm,
            discipline: &mut f.discipline,
            pause: &mut f.pause,
            end: &mut f.end,
            completion: &mut f.completion,
            capture: &mut f.capture,
            competition: &mut f.competition,
            delivery: &mut f.delivery,
            pre_origin_inputs: &mut f.pre,
        },
        NativeGameplayConfig {
            origin: point(1, 0),
            stream_origin: point(2, 0),
            playback_origin: point(2, 0),
            song_origin: Timestamp::ZERO,
            sample_rate: 1000,
            end_song: finite.then_some(Timestamp::from_nanos(10_000_000)),
            advance_lag: Duration::from_nanos(10_000_000),
            seconds: None,
            pause_supported: false,
            logical_schedule: true,
        },
    )
}
#[test]
fn empty_transfer_without_new_collector_cut_cannot_miss_or_advance_deadline() {
    for cuts in [vec![None; 8], vec![Some(point(1, 0)); 8]] {
        let mut f = Fixture::new(false, false);
        run_collector_cut(&mut f, false, true, cuts).unwrap();
        assert_eq!(f.device.step, 8);
        assert_eq!(
            f.runtime.judge().state(beatkernel::chart::ObjectId(1)),
            Some(beatkernel::interaction::InteractionState::Pending)
        );
        assert!(f
            .capture
            .as_ref()
            .unwrap()
            .records()
            .iter()
            .all(|r| r.song_time == Timestamp::ZERO));
        assert_eq!(f.delivery.observed_events(), 0);
    }
}
#[test]
fn finite_output_end_waits_for_collector_cut_instead_of_receipt() {
    let mut f = Fixture::new(true, false);
    run_collector_cut(&mut f, true, true, vec![None; 8]).unwrap();
    assert_eq!(f.device.step, 8);
    assert!(f.completion.is_none());
    assert!(f.capture.as_ref().unwrap().records().is_empty());
    assert!(f
        .device
        .report
        .unwrap()
        .playback_end_physical_frame
        .is_some());
}
#[test]
fn collector_cut_domains_regression_and_future_are_refused_before_deadline() {
    for cuts in [
        vec![Some(point(2, 0))],
        vec![Some(point(1, 10_000_001))],
        vec![Some(point(1, 10_000_000)), Some(point(1, 9_999_999))],
    ] {
        let mut f = Fixture::new(false, false);
        assert!(run_collector_cut(&mut f, false, true, cuts).is_err());
        assert_eq!(
            f.runtime.judge().state(beatkernel::chart::ObjectId(1)),
            Some(beatkernel::interaction::InteractionState::Pending)
        );
        assert_eq!(f.delivery.observed_events(), 0);
    }
}
#[test]
fn collector_cut_before_origin_is_withheld_without_retimestamping() {
    let mut f = Fixture::new(false, false);
    run_collector_cut(&mut f, false, true, vec![Some(point(1, -1)); 8]).unwrap();
    assert_eq!(
        f.runtime.judge().state(beatkernel::chart::ObjectId(1)),
        Some(beatkernel::interaction::InteractionState::Pending)
    );
    assert_eq!(f.pre, 0);
    assert!(f.capture.as_ref().unwrap().records().is_empty());
}
#[test]
fn future_event_is_checked_against_original_receipt_even_without_collector_cut() {
    let mut f = Fixture::new(false, false);
    f.device.invalid = true;
    assert!(run_collector_cut(&mut f, false, false, vec![None; 8]).is_err());
    assert_eq!(f.delivery.observed_events(), 0);
    assert!(f.capture.as_ref().unwrap().records().is_empty());
}

enum HandoffAction {
    Drain(Vec<PhysicalInputEvent>, ClockPoint),
    Fence(std::sync::mpsc::Sender<()>),
}
struct HandoffSource(std::sync::mpsc::Receiver<HandoffAction>);
impl crate::native_input::NativeInputSource for HandoffSource {
    fn service(
        &mut self,
        sink: &mut crate::native_input::InputPublisher<'_>,
        _: usize,
    ) -> Result<crate::native_input::SourceDrain, String> {
        let mut cut = None;
        match self.0.try_recv() {
            Ok(HandoffAction::Drain(events, completed)) => {
                for event in events {
                    sink.publish(event).map_err(|e| e.to_string())?;
                }
                cut = Some(completed);
            }
            Ok(HandoffAction::Fence(done)) => {
                done.send(()).unwrap();
            }
            Err(_) => {}
        }
        Ok(crate::native_input::SourceDrain {
            completed_through: cut,
            idle: true,
            closed: false,
        })
    }
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}
fn handoff_collector() -> (
    crate::native_input::NativeInputCollector,
    std::sync::mpsc::Sender<HandoffAction>,
) {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut collector = crate::native_input::NativeInputCollector::spawn(
        crate::native_input::CollectorConfig {
            domain: ClockDomainId(1),
            entries: 16,
            bytes: 65536,
            max_payload_bytes: 4096,
            service_quantum: 4,
            idle_wait: std::time::Duration::from_millis(1),
        },
        move || Ok(HandoffSource(rx)),
    )
    .unwrap();
    collector.wait_ready().unwrap();
    (collector, tx)
}
fn publish_handoff(
    collector: &crate::native_input::NativeInputCollector,
    tx: &std::sync::mpsc::Sender<HandoffAction>,
    events: Vec<PhysicalInputEvent>,
    cut: ClockPoint,
) {
    let (done, wait) = std::sync::mpsc::channel();
    tx.send(HandoffAction::Drain(events, cut)).unwrap();
    tx.send(HandoffAction::Fence(done)).unwrap();
    collector.wake();
    wait.recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
}
#[test]
fn same_collector_startup_retention_handoff_keeps_events_and_fifo_cut_exactly_once() {
    let (mut collector, tx) = handoff_collector();
    let original = vec![
        input(10, 1, ButtonState::Down),
        input(11, 2, ButtonState::Up),
    ];
    publish_handoff(&collector, &tx, original.clone(), point(1, 12));
    let mut owner = crate::native_gameplay::NativeCollectedInput::new().unwrap();
    let mut pre = 0;
    assert!(owner
        .service_start(&mut collector, true, &mut pre, 1)
        .unwrap());
    let mut events = VecDeque::new();
    let first = owner.acquire(&mut collector, &mut events, 1).unwrap();
    assert_eq!(events.iter().cloned().collect::<Vec<_>>(), original);
    assert_eq!(first.completed_through, None);
    let second = owner.acquire(&mut collector, &mut events, 1).unwrap();
    assert_eq!(second.completed_through, Some(point(1, 12)));
    assert_eq!(events.iter().cloned().collect::<Vec<_>>(), original);
    let third = owner.acquire(&mut collector, &mut events, 1).unwrap();
    assert_eq!(third.completed_through, None);
    assert_eq!(events.len(), 2);
    assert_eq!(pre, 0);
    collector.stop_and_join().unwrap();
}
#[test]
fn startup_discard_then_retain_preserves_existing_counts_and_same_worker() {
    let (mut collector, tx) = handoff_collector();
    publish_handoff(
        &collector,
        &tx,
        vec![input(1, 1, ButtonState::Down)],
        point(1, 2),
    );
    let mut owner = crate::native_gameplay::NativeCollectedInput::new().unwrap();
    let mut pre = 0;
    assert!(owner
        .service_start(&mut collector, false, &mut pre, 4)
        .unwrap());
    assert_eq!(pre, 1);
    let retained = input(3, 2, ButtonState::Up);
    publish_handoff(&collector, &tx, vec![retained.clone()], point(1, 4));
    assert!(owner
        .service_start(&mut collector, true, &mut pre, 4)
        .unwrap());
    let mut events = VecDeque::new();
    let batch = owner.acquire(&mut collector, &mut events, 4).unwrap();
    assert_eq!(events, VecDeque::from([retained]));
    assert_eq!(batch.completed_through, Some(point(1, 4)));
    assert_eq!(pre, 1);
    owner.acquire(&mut collector, &mut events, 4).unwrap();
    assert_eq!(events.len(), 1);
    collector.stop_and_join().unwrap();
}
