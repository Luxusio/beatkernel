//! Both actual pumps use the common owner/controller and original producer lease.
use crate::gameplay_output_owner::{GameplayOutputOwner, fixtures::*};
use crate::{
    gameplay_presentation::GameplayOutputContext,
    native_gameplay::{GameplaySession, run_gameplay_with_ports},
    native_cohort::{CohortSession, GameplayPlayerState, run_cohort_with_ports},
    local_runtime::{SoloRuntime, RuntimeGroup, MemberConfig},
    local_input::InputMerger,
    local_players::PlayerId,
    competition::ScoreSummary,
};
use std::{
    rc::Rc,
    cell::{RefCell, Cell},
};
struct Device {
    owner: GameplayOutputOwner<Backend>,
    trace: Rc<RefCell<Trace>>,
    devices: Vec<DeviceId>,
    command: Rc<Cell<bool>>,
    step: usize,
    published: usize,
    published_step: usize,
    waiting: Vec<(
        u64,
        Option<ClockPair>,
        Option<RenderReport>,
        crate::playback_pause::PausePhase,
    )>,
    seeds: Vec<(u64, ClockPair)>,
}
impl GameplayDevice for Device {
    type Presentation = PresentationEstimator;
    fn observe(&mut self, p: &mut PresentationEstimator) -> NativeGameplayResult<()> {
        self.step += 1;
        if self.step > 12 {
            return Err("owned output pump failed to terminate".into());
        }
        if let Some(output) = self.owner.current_mut() {
            output.render(10);
        }
        self.owner.observe(p)?;
        Ok(())
    }
    fn output_replacement_pending(&self) -> bool {
        self.owner.replacement_pending()
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(self.owner.render_report())
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(point(1, self.trace.borrow().frames as i64 * 1_000_000))
    }
    fn pause_observation(
        &mut self,
        pair: ClockPair,
    ) -> NativeGameplayResult<crate::live_pause::LivePauseObservation> {
        let now = self.host_now()?;
        Ok(self.owner.pause_observation(pair, now)?)
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        if self.step == 1 {
            self.command.set(true);
        }
        if self.step == 4 {
            assert!(self.owner.replacement_pending());
            self.command.set(false);
        }
        let input = match self.step {
            1 => Some((10_000_000, u64::MAX - 3, ButtonState::Down)),
            2 => Some((20_000_000, u64::MAX - 2, ButtonState::Up)),
            9 => Some((75_000_000, u64::MAX - 1, ButtonState::Down)),
            10 => Some((80_000_000, u64::MAX, ButtonState::Up)),
            _ => None,
        };
        if let Some((ns, sequence, state)) = input {
            for &device in self.devices.iter().rev() {
                retain_input(
                    events,
                    PhysicalInputEvent::Button(ButtonEvent {
                        meta: EventMeta::new(device, point(1, ns), sequence),
                        control: PhysicalControlId::keyboard(4),
                        state,
                    }),
                )?;
            }
        }
        Ok(InputBatch {
            backlog: false,
            closed: self.step >= 12,
        })
    }
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        p: &PresentationEstimator,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        self.owner.observe_end(end, p)
    }
    fn seed_resume(
        &mut self,
        p: &mut PresentationEstimator,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        self.owner.seed_resume(p)?;
        self.seeds.push((p.epoch(), p.latest_pair().unwrap()));
        Ok(())
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        Ok(pair_frame(self.trace.borrow().frames).source)
    }
    fn publish_paused_output(
        &mut self,
        context: GameplayOutputContext<'_, PresentationEstimator>,
    ) -> NativeGameplayResult<bool> {
        let old = (
            context.presentation.epoch(),
            context.presentation.latest_pair(),
            self.owner.render_report(),
            context.pause.phase(),
        );
        let now = self.host_now()?;
        let result = self.owner.publish_paused(context, now)?;
        if result {
            self.published += 1;
            self.published_step = self.step;
        } else if self.owner.replacement_pending() {
            self.waiting.push(old);
        }
        Ok(result)
    }
}
fn chart() -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(
        "#BPM 6000\n#WAV01 original.wav\n#00011:00010000\n",
        Default::default(),
    )
    .unwrap()
}
fn setup(
    devices: Vec<DeviceId>,
) -> (
    Device,
    CommandProducer,
    PresentationEstimator,
    NativePause,
    Host,
) {
    let (output, producer, trace) = initial(devices.clone());
    let mut owner = GameplayOutputOwner::new(
        Backend {
            trace: trace.clone(),
        },
        output,
    );
    assert!(owner.queue(request(71, 2), 100_000_000).is_ok());
    let command = Rc::new(Cell::new(false));
    let mut presentation =
        PresentationEstimator::new(settings(), point(2, 0), ClockDomainId(1), Timestamp::ZERO)
            .unwrap();
    presentation.observe_clock_pair(pair(0, 0)).unwrap();
    (
        Device {
            owner,
            trace,
            devices,
            command: command.clone(),
            step: 0,
            published: 0,
            published_step: 0,
            waiting: Vec::new(),
            seeds: Vec::new(),
        },
        producer,
        presentation,
        NativePause::new(point(2, 0), ClockDomainId(1), 1000).unwrap(),
        Host {
            pause_command: Some(command),
            ..Default::default()
        },
    )
}
fn capture(source: &beatkernel_bms::BmsChart) -> crate::replay_capture::LiveReplayCapture {
    crate::replay_capture::LiveReplayCapture::new(
        &judge(source),
        ClockDomainId(1),
        beatkernel::replay::codec::ReplayCodecLimits::new(
            65536,
            128,
            4096,
            beatkernel::input::CodecLimits::new(4096, 4096).unwrap(),
        )
        .unwrap(),
    )
    .unwrap()
}
fn assert_trace(
    device: &Device,
    p: &PresentationEstimator,
    pause: &NativePause,
    transport: &Transport,
) {
    assert_eq!(device.published, 1);
    assert_eq!(device.published_step, 5);
    assert_eq!(device.waiting.len(), 2);
    for (epoch, pair, report, phase) in &device.waiting {
        assert_eq!(*epoch, 0);
        assert_eq!(*pair, Some(pair_frame(30)));
        assert_eq!(report.unwrap().start_frame, 20);
        assert_eq!(*phase, crate::playback_pause::PausePhase::Paused);
    }
    assert_eq!(p.epoch(), 1);
    assert_eq!(p.config(), settings());
    assert_eq!(pause.epoch(), 1);
    assert_eq!(pause.phase(), crate::playback_pause::PausePhase::Running);
    assert_eq!(device.seeds, [(1, pair_frame(60))]);
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(10_000_000))
            .unwrap(),
        Timestamp::from_nanos(10_000_000)
    );
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(40_000_000))
            .unwrap(),
        Timestamp::from_nanos(20_000_000)
    );
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(60_000_000))
            .unwrap(),
        Timestamp::from_nanos(30_000_000)
    );
    let trace = device.trace.borrow();
    assert_eq!(trace.calls.iter().filter(|&&c| c == "open").count(), 1);
    assert_eq!(trace.calls.iter().filter(|&&c| c == "start").count(), 1);
    assert!(trace.observed_epochs.contains(&1));
    assert_eq!(trace.frames, trace.pcm.len() as u64);
}
fn assert_capture(
    capture: &crate::replay_capture::LiveReplayCapture,
    header: &beatkernel::replay::ReplayHeader,
    device: DeviceId,
) {
    assert_eq!(capture.header(), header);
    let inputs = capture
        .records()
        .iter()
        .filter_map(|r| match &r.operation {
            beatkernel::replay::ReplayOperation::Input(input) => Some(&input.physical),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!inputs.is_empty());
    assert!(inputs.iter().all(|e| e.meta().source == device));
    assert!(inputs.iter().any(|e| e.meta().sequence == u64::MAX - 3));
    assert!(inputs.iter().any(|e|matches!(e,PhysicalInputEvent::Button(b) if b.state==ButtonState::Up && b.meta.original_clock_point==Some(point(1,20_000_000)))));
    assert!(
        capture
            .records()
            .windows(2)
            .all(|r| r[0].song_time <= r[1].song_time)
    );
}
#[test]
fn actual_solo_owner_controller_waits_despite_early_resume_then_preserves_hit_gauge_capture_pcm_and_history()
 {
    let source = chart();
    let (mut device, producer, mut presentation, mut pause, mut host) =
        setup(vec![DeviceId(u64::MAX)]);
    let mut runtime = SoloRuntime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings(None),
        judge(&source),
        producer,
        vec![SoundBinding {
            object: source.compile().unwrap().chart.objects()[0].id,
            stage: beatkernel::judge::JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(u64::MAX),
            gain: 1.,
        }],
        8,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let mut gauge = BmsGauge::default();
    let mut bgm = bgm();
    let mut end = None;
    let mut completion = None;
    let mut capture = Some(capture(&source));
    let header = capture.as_ref().unwrap().header().clone();
    let mut observer = Some(Observer::default());
    let mut delivery = InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap();
    let mut pre = 0;
    run_gameplay_with_ports(
        &mut device,
        GameplaySession {
            runtime: &mut runtime,
            gauge: &mut gauge,
            bgm: &mut bgm,
            discipline: &mut presentation,
            pause: &mut pause,
            end: &mut end,
            completion: &mut completion,
            capture: &mut capture,
            competition: &mut observer,
            delivery: &mut delivery,
            pre_origin_inputs: &mut pre,
        },
        NativeGameplayConfig {
            pause_supported: true,
            ..config(false)
        },
        &mut Control::default(),
        &mut host,
    )
    .unwrap();
    assert_trace(&device, &presentation, &pause, runtime.transport_mut());
    assert_capture(capture.as_ref().unwrap(), &header, DeviceId(u64::MAX));
    assert_eq!(
        gauge.snapshot().level_units,
        21 * crate::gauge::GAUGE_UNITS_PER_PERCENT
    );
    assert_eq!(gauge.snapshot().failure, None);
    assert_eq!(observer.as_ref().unwrap().marks, 0);
    assert!(completion.is_none());
    assert_eq!(delivery.observed_events(), 4);
    assert_eq!(pre, 0);
    assert_eq!(
        host.solo
            .iter()
            .map(|r| r.judge_events.len())
            .sum::<usize>(),
        1
    );
    assert_eq!(&device.trace.borrow().pcm[10..12], &[0.25, 0.5]);
    assert!(device.owner.stop().is_ok());
}
#[test]
fn actual_cohort_owner_controller_defers_early_resume_and_keeps_original_roster_scores_gauges_captures_and_future_voices()
 {
    let source = chart();
    let ids = [
        (PlayerId(7), DeviceId(7)),
        (PlayerId(u32::MAX), DeviceId(u64::MAX)),
    ];
    let (mut device, producer, mut presentation, mut pause, mut host) =
        setup(ids.iter().map(|(_, d)| *d).collect());
    let members = ids
        .iter()
        .map(|(player, source_id)| MemberConfig {
            player: *player,
            device: Some(*source_id),
            bindings: bindings(Some(*source_id)),
            judge: judge(&source),
            sounds: vec![SoundBinding {
                object: source.compile().unwrap().chart.objects()[0].id,
                stage: beatkernel::judge::JudgeStage::Instant,
                sample: SampleId(1),
                voice: VoiceId(source_id.0),
                gain: 1.,
            }],
        })
        .collect();
    let mut group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        producer,
        members,
        8,
        &[],
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    let mut states = ids
        .iter()
        .map(|(player, _)| GameplayPlayerState {
            player: *player,
            capture: Some(capture(&source)),
            competition: Some(Observer::default()),
            completion: None,
            score: ScoreSummary::default(),
            gauge: BmsGauge::default(),
            last_song: Timestamp::ZERO,
        })
        .collect::<Vec<_>>();
    let headers = states
        .iter()
        .map(|s| s.capture.as_ref().unwrap().header().clone())
        .collect::<Vec<_>>();
    let mut merger = InputMerger::new(
        ClockDomainId(1),
        point(1, 0),
        ids.iter().map(|(_, d)| *d).collect(),
        16,
    )
    .unwrap();
    let mut bgm = bgm();
    let mut end = None;
    let mut network = GroupObserver::default();
    let mut delivery = InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap();
    let mut pre = 0;
    run_cohort_with_ports(
        &mut device,
        CohortSession {
            group: &mut group,
            network: Some(&mut network),
            states: &mut states,
            merger: &mut merger,
            bgm: &mut bgm,
            discipline: &mut presentation,
            pause: &mut pause,
            end: &mut end,
            delivery: &mut delivery,
            pre_origin_inputs: &mut pre,
        },
        NativeGameplayConfig {
            pause_supported: true,
            ..config(false)
        },
        &mut Control::default(),
        &mut host,
    )
    .unwrap();
    assert_trace(&device, &presentation, &pause, group.transport());
    assert_eq!(
        states.iter().map(|s| s.player).collect::<Vec<_>>(),
        [PlayerId(7), PlayerId(u32::MAX)]
    );
    assert!(
        network
            .rows
            .iter()
            .all(|row| row == &[PlayerId(7), PlayerId(u32::MAX)])
    );
    assert_eq!(network.marks, 0);
    assert_eq!(delivery.observed_events(), 8);
    assert_eq!(pre, 0);
    for (index, state) in states.iter().enumerate() {
        assert_capture(
            state.capture.as_ref().unwrap(),
            &headers[index],
            ids[index].1,
        );
        assert_eq!(
            (
                state.score.hits,
                state.score.misses,
                state.score.combo,
                state.score.max_combo
            ),
            (1, 0, 1, 1)
        );
        assert_eq!(
            state.gauge.snapshot().level_units,
            21 * crate::gauge::GAUGE_UNITS_PER_PERCENT
        );
        assert_eq!(state.gauge.snapshot().failure, None);
        assert_eq!(state.competition.as_ref().unwrap().marks, 0);
        assert!(state.completion.is_none());
    }
    assert_eq!(&device.trace.borrow().pcm[50..52], &[0.5, 1.]);
    assert!(device.owner.stop().is_ok());
}

mod live_output {
    include!("native_live_output_fixtures.rs");
}
