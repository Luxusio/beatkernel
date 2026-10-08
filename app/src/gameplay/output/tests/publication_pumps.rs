//! Deferred actual solo/cohort pumps with one uniquely moved memory output.
use crate::gameplay_presentation_port_fixtures::*;
use crate::{
    gameplay_presentation::{GameplayOutputContext, prepare_output_timing_rebind},
    output_replacement::{ReadyOutput, publish_ready_output},
    native_gameplay::{GameplaySession, run_gameplay_with_ports},
    native_cohort::{CohortSession, GameplayPlayerState, run_cohort_with_ports},
    local_runtime::{SoloRuntime, RuntimeGroup, MemberConfig},
    local_input::InputMerger,
    local_players::PlayerId,
    competition::ScoreSummary,
};
struct Output {
    mixer: Mixer,
    epoch: u64,
    report: Option<RenderReport>,
}
struct Device {
    output: Option<Output>,
    devices: Vec<DeviceId>,
    command: std::rc::Rc<std::cell::Cell<bool>>,
    step: usize,
    pcm: Vec<f32>,
    published: usize,
    hook_calls: usize,
    seeds: Vec<(u64, DisciplineConfig, ClockPair)>,
    origin: Option<ClockPoint>,
    failed_ready: Option<ReadyOutput<PresentationEstimator, Output>>,
}
fn pair_frame(frame: u64) -> ClockPair {
    pair(frame as i64 * 1_000_000, frame as i64 * 1_000_000)
}
impl GameplayDevice for Device {
    type Presentation = PresentationEstimator;
    fn observe(&mut self, p: &mut PresentationEstimator) -> NativeGameplayResult<()> {
        self.step += 1;
        if self.step > 12 {
            return Err("memory publication pump did not terminate".into());
        }
        let output = self.output.as_mut().unwrap();
        let mut pcm = [0.; 10];
        output.report = Some(output.mixer.render(&mut pcm)?);
        self.pcm.extend_from_slice(&pcm);
        p.observe_clock_pair_in_epoch(output.epoch, pair_frame(output.mixer.frame_cursor()))?;
        Ok(())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(self.output.as_ref().unwrap().report)
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(pair_frame(self.output.as_ref().unwrap().mixer.frame_cursor()).target)
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        if self.step == 1 {
            self.command.set(true);
        }
        if self.step == 4 {
            self.command.set(false);
        }
        let input = match self.step {
            1 => Some((10_000_000, u64::MAX - 3, ButtonState::Down)),
            2 => Some((20_000_000, u64::MAX - 2, ButtonState::Up)),
            7 => Some((75_000_000, u64::MAX - 1, ButtonState::Down)),
            8 => Some((80_000_000, u64::MAX, ButtonState::Up)),
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
            completed_through: Some(self.host_now()?),
            backlog: false,
            closed: self.step >= 10,
        })
    }
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        p: &PresentationEstimator,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        Ok(end.observe(report, p.latest_pair().unwrap())?)
    }
    fn seed_resume(
        &mut self,
        p: &mut PresentationEstimator,
        reference: ClockPair,
    ) -> NativeGameplayResult<()> {
        let epoch = self.output.as_ref().unwrap().epoch;
        p.observe_clock_pair_in_epoch(epoch, reference)?;
        self.seeds.push((epoch, p.config(), reference));
        Ok(())
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        Ok(pair_frame(self.output.as_ref().unwrap().mixer.frame_cursor()).source)
    }
    fn publish_paused_output(
        &mut self,
        mut context: GameplayOutputContext<'_, PresentationEstimator>,
    ) -> NativeGameplayResult<bool> {
        self.hook_calls += 1;
        if self.published != 0 {
            return Ok(false);
        }
        let hold = context.control.hold_audio_pause()?;
        let mut output = self.output.take().unwrap();
        let epoch = context.presentation.epoch() + 1;
        let mut timing = prepare_output_timing_rebind(
            context.presentation,
            context.pause,
            epoch,
            &output.mixer,
            context.config.song_origin,
        )?;
        let mut pcm = [0.; 10];
        let report = output.mixer.render(&mut pcm)?;
        self.pcm.extend_from_slice(&pcm);
        output.report = Some(report);
        output.epoch = epoch;
        let accepted = pair_frame(output.mixer.frame_cursor());
        timing
            .presentation
            .observe_clock_pair_in_epoch(epoch, accepted)?;
        timing
            .pause
            .observe_in_epoch(epoch, Some(report), accepted)?;
        let origin = timing.playback_origin;
        let ready = ReadyOutput {
            output,
            timing,
            hold,
        };
        match publish_ready_output(ready, &mut self.output, context) {
            Ok(()) => {
                self.published += 1;
                self.origin = Some(origin);
                Ok(true)
            }
            Err(failure) => {
                self.failed_ready = Some(failure.ready);
                Err(failure.error)
            }
        }
    }
}
fn chart() -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(
        "#BPM 6000\n#WAV01 original.wav\n#00011:00010000\n",
        Default::default(),
    )
    .unwrap()
}
fn settings() -> DisciplineConfig {
    DisciplineConfig {
        capacity: 8,
        min_span: Duration::from_nanos(500_000_000),
        ..Default::default()
    }
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
    let (memory, producer) = device(false, devices.clone());
    let command = std::rc::Rc::new(std::cell::Cell::new(false));
    let output = Output {
        mixer: memory.mixer,
        epoch: 0,
        report: None,
    };
    let device = Device {
        output: Some(output),
        devices,
        command: command.clone(),
        step: 0,
        pcm: Vec::new(),
        published: 0,
        hook_calls: 0,
        seeds: Vec::new(),
        origin: None,
        failed_ready: None,
    };
    let mut presentation =
        PresentationEstimator::new(settings(), point(2, 0), ClockDomainId(1), Timestamp::ZERO)
            .unwrap();
    presentation.observe_clock_pair(pair(0, 0)).unwrap();
    (
        device,
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
fn assert_clocks(
    device: &Device,
    presentation: &PresentationEstimator,
    pause: &NativePause,
    transport: &Transport,
) {
    assert_eq!(device.published, 1);
    assert!(device.hook_calls >= 1);
    assert_eq!(device.origin, Some(point(2, 30_000_000)));
    assert!(device.failed_ready.is_none());
    assert_eq!(presentation.epoch(), 1);
    assert_eq!(pause.epoch(), 1);
    assert_eq!(presentation.config(), settings());
    assert_eq!(device.seeds.len(), 1);
    assert_eq!(device.seeds[0].0, 1);
    assert_eq!(device.seeds[0].1, settings());
    assert_eq!(device.seeds[0].2, pair(70_000_000, 70_000_000));
    assert_eq!(pause.phase(), crate::playback_pause::PausePhase::Running);
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(10_000_000))
            .unwrap(),
        Timestamp::from_nanos(10_000_000)
    );
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(50_000_000))
            .unwrap(),
        Timestamp::from_nanos(20_000_000)
    );
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(70_000_000))
            .unwrap(),
        Timestamp::from_nanos(30_000_000)
    );
    assert_eq!(
        device.output.as_ref().unwrap().mixer.frame_cursor(),
        device.pcm.len() as u64
    );
}
fn assert_capture(
    capture: &crate::replay_capture::LiveReplayCapture,
    header: &beatkernel::replay::ReplayHeader,
    device: DeviceId,
) {
    assert_eq!(capture.header(), header);
    assert!(!capture.records().is_empty());
    let inputs = capture
        .records()
        .iter()
        .filter_map(|r| match &r.operation {
            beatkernel::replay::ReplayOperation::Input(input) => Some(&input.physical),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(inputs.iter().all(|e| e.meta().source == device));
    assert!(inputs.iter().any(|e| e.meta().sequence == u64::MAX - 3));
    assert!(inputs.iter().any(|e|matches!(e,PhysicalInputEvent::Button(b) if b.state==ButtonState::Up && b.meta.original_clock_point==Some(point(1,20_000_000)))));
    assert!(
        capture
            .records()
            .windows(2)
            .all(|rows| rows[0].song_time <= rows[1].song_time)
    );
}
#[test]
fn actual_solo_pump_publishes_new_output_once_and_preserves_original_hit_gauge_capture_and_transport_history_on_resume()
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
    assert_clocks(&device, &presentation, &pause, runtime.transport_mut());
    assert_capture(capture.as_ref().unwrap(), &header, DeviceId(u64::MAX));
    assert_eq!(
        gauge.snapshot().level_units,
        21 * crate::gauge::GAUGE_UNITS_PER_PERCENT
    );
    assert_eq!(gauge.snapshot().failure, None);
    assert_eq!(observer.as_ref().unwrap().marks, 0);
    assert!(completion.is_none());
    assert_eq!(pre, 0);
    assert_eq!(delivery.observed_events(), 4);
    assert_eq!(
        host.solo
            .iter()
            .map(|r| r.judge_events.len())
            .sum::<usize>(),
        1
    );
    assert_eq!(&device.pcm[10..12], &[0.25, 0.5]);
    assert!(host.pause_states.contains(&PauseState::Resuming));
}
#[test]
fn actual_cohort_pump_retains_both_original_roster_scores_gauges_captures_and_shared_output_epoch_without_session_rebuild()
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
    assert_clocks(&device, &presentation, &pause, group.transport());
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
    assert_eq!(&device.pcm[60..62], &[0.5, 1.]);
    assert!(host.pause_states.contains(&PauseState::Resuming));
}
