use super::*;
use crate::gameplay_output_ui::{GameplayOutputUi, OutputUiPort};
use crate::live_output_control::{OutputControls, OutputCapability, OutputRequest, OutputReply};
use crate::settings::SettingsHost;
struct UiState {
    controls: OutputControls,
    blocked: bool,
    replies: Vec<OutputReply>,
}
#[derive(Clone)]
struct Ui(Rc<RefCell<UiState>>);
impl OutputUiPort for Ui {
    fn advertise(&mut self, cap: Option<OutputCapability>) -> std::io::Result<()> {
        self.0
            .borrow_mut()
            .controls
            .advertise(cap)
            .map_err(std::io::Error::other)
    }
    fn take_request(&mut self) -> std::io::Result<Option<OutputRequest>> {
        Ok(self.0.borrow_mut().controls.take_request())
    }
    fn reply(&mut self, reply: &OutputReply) -> std::io::Result<()> {
        let mut state = self.0.borrow_mut();
        if state.blocked {
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        state.controls.reply(reply).map_err(std::io::Error::other)?;
        state.replies.push(reply.clone());
        Ok(())
    }
    fn pending(&self) -> bool {
        self.0.borrow().controls.pending()
    }
}
fn cap(epoch: u64, period: usize) -> OutputCapability {
    OutputCapability {
        host: SettingsHost::Linux,
        current_args: vec![
            "--alsa".into(),
            format!("memory-epoch-{epoch}"),
            "--buffer-frames".into(),
            (period * 4).to_string(),
            "--period-frames".into(),
            period.to_string(),
        ],
    }
}
fn map_request(
    req: &OutputRequest,
    out: &crate::gameplay_output_owner::fixtures::Output,
) -> Result<crate::gameplay_output_owner::fixtures::Request, String> {
    crate::settings::NativeSettings::output_only(&req.args, SettingsHost::Linux)?;
    #[cfg(target_os = "linux")]
    {
        use beatkernel_platform::{
            linux::AlsaRequest,
            audio::{DeviceFormat, SampleEncoding},
        };
        let format = out.mixer.as_ref().unwrap().config().format();
        let current = AlsaRequest {
            device: format!("memory-epoch-{}", out.epoch),
            format: DeviceFormat::new(
                format.sample_rate(),
                format.channels(),
                SampleEncoding::Float32,
                None,
            )
            .unwrap(),
            buffer_frames: 40,
            period_frames: 10,
            allow_size_rounding: false,
            monotonic_domain: ClockDomainId(1),
        };
        crate::native_alsa_output_ui::request_for_args(&current, &req.args)?;
    }
    #[cfg(not(target_os = "linux"))]
    let _ = out;
    Ok(request(req.id, 2))
}
struct ControlledDevice {
    inner: Device,
    ui: GameplayOutputUi<Ui>,
    state: Rc<RefCell<UiState>>,
    acks: Vec<(usize, OutputReply)>,
}
impl GameplayDevice for ControlledDevice {
    type Presentation = PresentationEstimator;
    fn observe(&mut self, p: &mut PresentationEstimator) -> NativeGameplayResult<()> {
        self.inner.observe(p)
    }
    fn output_replacement_pending(&self) -> bool {
        self.inner.owner.replacement_pending() || self.ui.pending()
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        self.inner.render_report()
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        self.inner.host_now()
    }
    fn pause_observation(
        &mut self,
        pair: ClockPair,
    ) -> NativeGameplayResult<crate::live_pause::LivePauseObservation> {
        self.inner.pause_observation(pair)
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        let result = self.inner.acquire(events)?;
        if self.inner.step == 7 {
            self.state.borrow_mut().blocked = false;
        }
        Ok(result)
    }
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        p: &PresentationEstimator,
        r: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        self.inner.observe_end(end, p, r)
    }
    fn seed_resume(
        &mut self,
        p: &mut PresentationEstimator,
        r: ClockPair,
    ) -> NativeGameplayResult<()> {
        self.inner.seed_resume(p, r)
    }
    fn fallback_schedule(&mut self, rate: u32) -> NativeGameplayResult<ClockPoint> {
        self.inner.fallback_schedule(rate)
    }
    fn publish_paused_output(
        &mut self,
        context: GameplayOutputContext<'_, PresentationEstimator>,
    ) -> NativeGameplayResult<bool> {
        let old = (
            context.presentation.epoch(),
            context.presentation.latest_pair(),
            self.inner.owner.render_report(),
            context.pause.phase(),
        );
        let now = self.inner.host_now()?;
        let published = self.ui.service(
            &mut self.inner.owner,
            context,
            now,
            &mut map_request,
            &mut |out| {
                let report = out.report.ok_or("missing actual applied block")?;
                Ok(cap(out.epoch, report.frames))
            },
        )?;
        if published {
            self.inner.published += 1;
            self.inner.published_step = self.inner.step;
            assert!(self.state.borrow().replies.is_empty());
        } else if self.inner.owner.replacement_pending() {
            self.inner.waiting.push(old);
        }
        if let Some(reply) = self.state.borrow_mut().controls.take_reply() {
            self.acks.push((self.inner.step, reply));
        }
        Ok(published)
    }
}
fn controlled(
    devices: Vec<DeviceId>,
) -> (
    ControlledDevice,
    CommandProducer,
    PresentationEstimator,
    NativePause,
    Host,
) {
    let (mut inner, producer, presentation, pause, host) = setup(devices);
    assert!(matches!(inner.owner.cancel(), Ok(false))); // Remove original queued test request; retain current output.
    let state = Rc::new(RefCell::new(UiState {
        controls: OutputControls::new(),
        blocked: true,
        replies: Vec::new(),
    }));
    let mut bridge = GameplayOutputUi::new(Ui(state.clone()));
    bridge.advertise(Some(cap(0, 10))).unwrap();
    assert_eq!(
        state
            .borrow_mut()
            .controls
            .request(vec![
                "--buffer-frames".into(),
                "40".into(),
                "--period-frames".into(),
                "10".into()
            ])
            .unwrap(),
        1
    );
    (
        ControlledDevice {
            inner,
            ui: bridge,
            state,
            acks: Vec::new(),
        },
        producer,
        presentation,
        pause,
        host,
    )
}
fn assert_controlled(
    device: &ControlledDevice,
    p: &PresentationEstimator,
    pause: &NativePause,
    transport: &Transport,
) {
    assert_eq!(device.inner.published, 1);
    assert_eq!(device.inner.published_step, 5);
    assert_eq!(device.acks.len(), 1);
    assert_eq!(device.acks[0].0, 7);
    assert_eq!(device.acks[0].1.id, 1);
    assert_eq!(device.acks[0].1.result.as_ref().unwrap(), &cap(1, 10));
    assert!(!device.ui.pending());
    assert_eq!(device.inner.waiting.len(), 2);
    for (epoch, pair, _, phase) in &device.inner.waiting {
        assert_eq!(*epoch, 0);
        assert_eq!(*pair, Some(pair_frame(30)));
        assert_eq!(*phase, crate::playback_pause::PausePhase::Paused);
    }
    assert_eq!(device.inner.seeds, [(1, pair_frame(80))]);
    assert_eq!(p.epoch(), 1);
    assert_eq!(pause.epoch(), 1);
    assert_eq!(pause.phase(), crate::playback_pause::PausePhase::Running);
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(60_000_000))
            .unwrap(),
        Timestamp::from_nanos(20_000_000)
    );
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(80_000_000))
            .unwrap(),
        Timestamp::from_nanos(30_000_000)
    );
    let trace = device.inner.trace.borrow();
    assert_eq!(trace.calls.iter().filter(|&&c| c == "open").count(), 1);
    assert_eq!(trace.calls.iter().filter(|&&c| c == "start").count(), 1);
}
#[test]
fn actual_solo_ui_bridge_acknowledges_only_published_ready_and_defers_early_resume_through_reply_contention()
 {
    let source = chart();
    let (mut device, producer, mut presentation, mut pause, mut host) =
        controlled(vec![DeviceId(u64::MAX)]);
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
    assert_controlled(&device, &presentation, &pause, runtime.transport_mut());
    assert_capture(capture.as_ref().unwrap(), &header, DeviceId(u64::MAX));
    assert_eq!(
        gauge.snapshot().level_units,
        21 * crate::gauge::GAUGE_UNITS_PER_PERCENT
    );
    assert_eq!(delivery.observed_events(), 4);
    assert_eq!(observer.as_ref().unwrap().marks, 0);
    assert!(completion.is_none());
    assert!(device.inner.owner.stop().is_ok());
}
#[test]
fn actual_cohort_ui_bridge_keeps_roster_scores_and_captures_while_waiting_for_ready_and_delayed_reply()
 {
    let source = chart();
    let ids = [
        (PlayerId(7), DeviceId(7)),
        (PlayerId(u32::MAX), DeviceId(u64::MAX)),
    ];
    let (mut device, producer, mut presentation, mut pause, mut host) =
        controlled(ids.iter().map(|(_, d)| *d).collect());
    let members = ids
        .iter()
        .map(|(player, id)| MemberConfig {
            player: *player,
            device: Some(*id),
            bindings: bindings(Some(*id)),
            judge: judge(&source),
            sounds: vec![SoundBinding {
                object: source.compile().unwrap().chart.objects()[0].id,
                stage: beatkernel::judge::JudgeStage::Instant,
                sample: SampleId(1),
                voice: VoiceId(id.0),
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
    assert_controlled(&device, &presentation, &pause, group.transport());
    assert_eq!(
        states.iter().map(|s| s.player).collect::<Vec<_>>(),
        [PlayerId(7), PlayerId(u32::MAX)]
    );
    for (index, state) in states.iter().enumerate() {
        assert_capture(
            state.capture.as_ref().unwrap(),
            &headers[index],
            ids[index].1,
        );
        assert_eq!((state.score.hits, state.score.misses), (1, 0));
        assert_eq!(
            state.gauge.snapshot().level_units,
            21 * crate::gauge::GAUGE_UNITS_PER_PERCENT
        );
    }
    assert_eq!(delivery.observed_events(), 8);
    assert_eq!(network.marks, 0);
    assert_eq!(&device.inner.trace.borrow().pcm[70..72], &[0.5, 1.]);
    assert!(device.inner.owner.stop().is_ok());
}
#[cfg(target_os = "linux")]
#[test]
fn actual_alsa_mapper_preserves_format_grid_rounding_and_accepts_larger_period_within_scalar_render_limit()
 {
    use beatkernel_platform::{
        linux::AlsaRequest,
        audio::{DeviceFormat, SampleEncoding},
    };
    let current = AlsaRequest {
        device: "hw:fixture".into(),
        format: DeviceFormat::new(48_000, 2, SampleEncoding::Float32, None).unwrap(),
        buffer_frames: 256,
        period_frames: 64,
        allow_size_rounding: true,
        monotonic_domain: ClockDomainId(u32::MAX),
    };
    let unchanged = crate::native_alsa_output_ui::request_for_args(&current, &[]).unwrap();
    assert_eq!(unchanged, current);
    let values = vec![
        "--alsa".into(),
        "default".into(),
        "--buffer-frames".into(),
        "8192".into(),
        "--period-frames".into(),
        "4096".into(),
    ];
    let updated = crate::native_alsa_output_ui::request_for_args(&current, &values).unwrap();
    assert_eq!(updated.period_frames, 4096);
    assert_eq!(updated.buffer_frames, 8192);
    assert_eq!(updated.device, "default");
    assert_eq!(updated.format, current.format);
    assert_eq!(updated.monotonic_domain, current.monotonic_domain);
    assert_eq!(updated.allow_size_rounding, current.allow_size_rounding);
}
#[cfg(target_os = "linux")]
#[test]
fn actual_alsa_mapper_rejects_malformed_sizes_and_unsupported_fields_without_output_effects() {
    use beatkernel_platform::{
        linux::AlsaRequest,
        audio::{DeviceFormat, SampleEncoding},
    };
    let current = AlsaRequest {
        device: "default".into(),
        format: DeviceFormat::new(48_000, 2, SampleEncoding::Float32, None).unwrap(),
        buffer_frames: 256,
        period_frames: 64,
        allow_size_rounding: false,
        monotonic_domain: ClockDomainId(7),
    };
    for values in [
        vec!["--period-frames", "0"],
        vec!["--period-frames", "256"],
        vec!["--buffer-frames", "0"],
        vec!["--period-frames", "-1"],
        vec!["--period-frames", "1.5"],
        vec!["--period-frames", "4294967296"],
        vec!["--sample-rate", "44100"],
        vec!["--chart", "other.bms"],
        vec!["--alsa", "bad\0name"],
    ] {
        let before = current.clone();
        assert!(
            crate::native_alsa_output_ui::request_for_args(
                &current,
                &values.into_iter().map(str::to_owned).collect::<Vec<_>>()
            )
            .is_err()
        );
        assert_eq!(current, before);
    }
}
#[cfg(target_os = "linux")]
#[test]
fn actual_bridge_numeric_refusal_sends_one_correlated_error_before_any_retirement_or_open_effect() {
    let (mut inner, mut producer, mut p, mut pause, _) = setup(vec![DeviceId(u64::MAX)]);
    assert!(matches!(inner.owner.cancel(), Ok(false)));
    inner.owner.current_mut().unwrap().render(2);
    inner.owner.observe(&mut p).unwrap();
    pause.request(true, pair_frame(2)).unwrap();
    producer.request_pause(true);
    inner.owner.current_mut().unwrap().render(1);
    inner.owner.observe(&mut p).unwrap();
    pause
        .observe(inner.owner.render_report(), pair_frame(3))
        .unwrap()
        .unwrap();
    let mut runtime = SoloRuntime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings(None),
        judge(&chart()),
        producer,
        vec![],
        8,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let state = Rc::new(RefCell::new(UiState {
        controls: OutputControls::new(),
        blocked: false,
        replies: Vec::new(),
    }));
    let mut bridge = GameplayOutputUi::new(Ui(state.clone()));
    bridge.advertise(Some(cap(0, 10))).unwrap();
    assert_eq!(
        state
            .borrow_mut()
            .controls
            .request(vec!["--period-frames".into(), "0".into()])
            .unwrap(),
        1
    );
    let mut config = config(false);
    let mut end = None;
    let result = bridge
        .service(
            &mut inner.owner,
            GameplayOutputContext {
                presentation: &mut p,
                pause: &mut pause,
                config: &mut config,
                end: &mut end,
                control: crate::gameplay_presentation::GameplayPauseControl::solo(&mut runtime),
            },
            point(1, 3_000_000),
            &mut map_request,
            &mut |_| Ok(cap(0, 10)),
        )
        .unwrap();
    assert!(!result);
    assert_eq!(state.borrow().replies.len(), 1);
    assert_eq!(state.borrow().replies[0].id, 1);
    assert!(state.borrow().replies[0].result.is_err());
    assert!(
        inner
            .trace
            .borrow()
            .calls
            .iter()
            .all(|c| !matches!(*c, "retire" | "open" | "start" | "take"))
    );
    assert_eq!(inner.owner.current().unwrap().epoch, 0);
    assert_eq!(p.epoch(), 0);
    assert_eq!(config.stream_origin, point(2, 0));
    assert!(inner.owner.stop().is_ok());
}
