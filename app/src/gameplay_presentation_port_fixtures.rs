//! Deferred business presentation contracts: no native backend or real clocks.
pub(crate) use beatkernel::{
    audio::*,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    runtime::{RuntimeProcessingClock, RuntimeReport, SoundBinding},
    telemetry::InputDeliveryTelemetry,
    time::{
        presentation::{DisciplineConfig, DisciplineUpdate, EstimatorError, PresentationEstimator},
        ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Duration, Timestamp,
    },
    transport::{Rate, Transport},
};
pub(crate) use crate::{
    bgm::{BgmConfig, BgmFeeder},
    gameplay_competition::{GroupCompetitionPort, SoloCompetitionPort},
    gameplay_presentation::{GameplayDevice, GameplayPresentationPort},
    gauge::BmsGauge,
    native_end::{EndBoundary, NativeEnd},
    native_gameplay::{InputBatch, NativeGameplayConfig, NativeGameplayResult, retain_input},
    native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost, PauseState},
    native_pump_control::NativePumpControl,
    playback_pause::NativePause,
};
pub(crate) use std::collections::VecDeque;

pub(crate) fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
pub(crate) fn pair(output: i64, host: i64) -> ClockPair {
    ClockPair {
        source: point(2, output),
        target: point(1, host),
    }
}
pub(crate) fn estimator() -> PresentationEstimator {
    let mut result = <PresentationEstimator as GameplayPresentationPort>::new_with_playback_origin(
        DisciplineConfig::default(),
        point(2, 0),
        point(2, 0),
        ClockDomainId(1),
        Timestamp::ZERO,
    )
    .unwrap();
    result.observe_clock_pair(pair(0, 0)).unwrap();
    result
}
pub(crate) fn config(finite: bool) -> NativeGameplayConfig {
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
    }
}
pub(crate) fn judge(source: &beatkernel_bms::BmsChart) -> JudgeEngine {
    JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap()
}
pub(crate) fn source() -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(
        "#BPM 3000\n#WAV01 original.wav\n#00011:00010000\n",
        Default::default(),
    )
    .unwrap()
}
pub(crate) fn bindings(device: Option<DeviceId>) -> BindingMap {
    BindingMap::from_bindings([Binding {
        device: device.map_or(DeviceSelector::Any, DeviceSelector::Exact),
        physical: PhysicalControlId::keyboard(4),
        game_control: GameControlId(0x11),
    }])
    .unwrap()
}
pub(crate) fn bgm() -> BgmFeeder {
    BgmFeeder::new(
        Vec::new(),
        BgmConfig {
            output_origin: point(2, 0),
            sample_rate: 1000,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(1_000_000_000),
            max_pending: 4,
        },
    )
    .unwrap()
}
pub(crate) fn pause_end(finite: bool) -> (NativePause, Option<NativeEnd>) {
    let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 1000).unwrap();
    let end = if finite {
        pause = pause.with_playback_end_frame(10).unwrap();
        let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 1000, 10).unwrap();
        end.observe(None, pair(0, 0)).unwrap();
        Some(end)
    } else {
        None
    };
    (pause, end)
}
pub(crate) struct MemoryDevice {
    pub mixer: Mixer,
    pub report: Option<RenderReport>,
    pub step: usize,
    pub pcm: Vec<f32>,
    pub devices: Vec<DeviceId>,
    pub close: bool,
    pub reject_at: Option<usize>,
    pub stale: bool,
    pub seeds: Vec<ClockPair>,
    pub pause_command: Option<std::rc::Rc<std::cell::Cell<bool>>>,
}
pub(crate) fn device(finite: bool, devices: Vec<DeviceId>) -> (MemoryDevice, CommandProducer) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5], limits).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let config = MixerConfig::new(
        format,
        ClockDomainId(2),
        Timestamp::ZERO,
        AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
    );
    let mixer = Mixer::new(
        if finite {
            config.with_playback_end_frame(10)
        } else {
            config
        },
        bank,
        consumer,
    )
    .unwrap();
    (
        MemoryDevice {
            mixer,
            report: None,
            step: 0,
            pcm: Vec::new(),
            devices,
            close: !finite,
            reject_at: None,
            stale: false,
            seeds: Vec::new(),
            pause_command: None,
        },
        producer,
    )
}
impl GameplayDevice for MemoryDevice {
    type Presentation = PresentationEstimator;
    fn observe(&mut self, presentation: &mut Self::Presentation) -> NativeGameplayResult<()> {
        self.step += 1;
        if self.step > 12 {
            return Err("pure pump failed to finish".into());
        }
        let mut pcm = [0.0; 10];
        self.report = Some(self.mixer.render(&mut pcm)?);
        self.pcm.extend_from_slice(&pcm);
        if self.reject_at == Some(self.step) {
            // A genuinely rejected pair, preserving the original accepted evidence.
            presentation.observe_clock_pair(pair(-1, self.step as i64 * 10_000_000))?;
        } else if !self.stale {
            presentation.observe_clock_pair(pair(
                self.step as i64 * 10_000_000,
                self.step as i64 * 10_000_000,
            ))?;
        }
        Ok(())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(self.report)
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(point(
            1,
            if self.stale {
                2_000_000_001
            } else {
                self.step as i64 * 10_000_000
            },
        ))
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        if let Some(command) = &self.pause_command {
            match self.step {
                1 => command.set(true),
                3 => command.set(false),
                _ => {}
            }
            if let Some((ns, sequence, state)) = match self.step {
                1 => Some((10_000_000, 1, ButtonState::Down)),
                2 => Some((20_000_000, 2, ButtonState::Up)),
                5 => Some((40_000_000, 3, ButtonState::Down)),
                6 => Some((45_000_000, 4, ButtonState::Up)),
                _ => None,
            } {
                for &device in &self.devices {
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
            return Ok(InputBatch {
                completed_through: Some(self.host_now()?),
                backlog: self.step == 5,
                closed: self.close && self.step >= 7,
            });
        }
        if self.step == 2 {
            for &device in self.devices.iter().rev() {
                retain_input(
                    events,
                    PhysicalInputEvent::Button(ButtonEvent {
                        meta: EventMeta::new(device, point(1, 20_000_000), u64::MAX - 1),
                        control: PhysicalControlId::keyboard(4),
                        state: ButtonState::Down,
                    }),
                )?;
            }
        }
        Ok(InputBatch {
            completed_through: Some(self.host_now()?),
            backlog: self.step == 2,
            closed: self.close && self.step >= 4,
        })
    }
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        presentation: &Self::Presentation,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        Ok(end.observe(report, presentation.latest_pair().unwrap())?)
    }
    fn seed_resume(
        &mut self,
        presentation: &mut Self::Presentation,
        reference: ClockPair,
    ) -> NativeGameplayResult<()> {
        presentation.observe_clock_pair(reference)?;
        self.seeds.push(reference);
        Ok(())
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        Ok(point(2, self.step as i64 * 10_000_000))
    }
}
#[derive(Default)]
pub(crate) struct Control {
    pub waits: usize,
}
impl NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        Err("unbounded pump must not read a wall clock".into())
    }
    fn checked_add(moment: u64, duration: std::time::Duration) -> Option<u64> {
        moment.checked_add(u64::try_from(duration.as_nanos()).ok()?)
    }
    fn wait(&mut self, duration: std::time::Duration) -> NativeGameplayResult<()> {
        assert_eq!(duration, std::time::Duration::from_millis(1));
        self.waits += 1;
        Ok(())
    }
}
#[derive(Default)]
pub(crate) struct Host {
    pub cancel: bool,
    pub solo: Vec<RuntimeReport>,
    pub local: Vec<Vec<(crate::local_players::PlayerId, i64, usize)>>,
    pub ends: Vec<Timestamp>,
    pub pause_command: Option<std::rc::Rc<std::cell::Cell<bool>>>,
    pub pause_states: Vec<PauseState>,
}
impl NativeGameplayHost for Host {
    fn cancelled(&self) -> bool {
        self.cancel
    }
    fn pause_requested(&self) -> bool {
        self.pause_command.as_ref().is_some_and(|value| value.get())
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, state: PauseState) {
        self.pause_states.push(state);
    }
    fn publish_section_end(&mut self, at: Timestamp) {
        self.ends.push(at);
    }
    fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        self.solo.push(report.clone());
        Ok(())
    }
    fn publish_local_reports(
        &mut self,
        reports: &[crate::local_runtime::PlayerReport],
    ) -> NativeGameplayResult<()> {
        self.local.push(
            reports
                .iter()
                .map(|r| {
                    (
                        r.player,
                        r.report.song_time.as_nanos(),
                        r.report.judge_events.len(),
                    )
                })
                .collect(),
        );
        Ok(())
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
}
#[derive(Default)]
pub(crate) struct Observer {
    pub marks: usize,
    pub times: Vec<i64>,
}
impl SoloCompetitionPort for Observer {
    fn observe(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        self.times.push(report.song_time.as_nanos());
        Ok(())
    }
    fn mark_native_completed(&mut self) {
        self.marks += 1;
    }
}
#[derive(Default)]
pub(crate) struct GroupObserver {
    pub marks: usize,
    pub rows: Vec<Vec<crate::local_players::PlayerId>>,
}
impl GroupCompetitionPort for GroupObserver {
    fn observe(
        &mut self,
        members: &[crate::multiplayer_group::MemberProgress],
    ) -> NativeGameplayResult<()> {
        self.rows.push(members.iter().map(|m| m.player).collect());
        Ok(())
    }
    fn mark_native_completed(&mut self) {
        self.marks += 1;
    }
}

#[test]
fn pure_port_preserves_literal_drift_and_historical_transport() {
    let mut e = estimator();
    e.observe_clock_pair(pair(1_000_250_000, 1_000_000_000))
        .unwrap();
    let mut transport = Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL);
    assert_eq!(
        GameplayPresentationPort::update(&mut e, point(1, 1_000_000_000), &mut transport).unwrap(),
        DisciplineUpdate::Applied {
            base_rate_ppm: 250,
            correction_ppm: 25,
            applied_rate_ppm: 275,
            phase_error_ns: 250_000,
            limited: false
        }
    );
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(500_000_000))
            .unwrap(),
        Timestamp::from_nanos(500_000_000)
    );
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(2_000_000_000))
            .unwrap(),
        Timestamp::from_nanos(2_000_275_000)
    );
    assert_eq!(
        GameplayPresentationPort::quality(&e),
        ClockMappingQuality::Unknown
    );
    assert_eq!(
        GameplayPresentationPort::latest_pair(&e),
        Some(pair(1_000_250_000, 1_000_000_000))
    );
}
#[test]
fn pure_port_refuses_stale_and_failed_update_without_mutating_evidence() {
    let mut e = estimator();
    e.observe_clock_pair(pair(1_000_000_000, 1_000_000_000))
        .unwrap();
    let accepted = GameplayPresentationPort::latest_pair(&e);
    assert_eq!(
        e.observe_clock_pair(pair(999_999_999, 2_000_000_000)),
        Err(EstimatorError::NonIncreasing)
    );
    assert_eq!(GameplayPresentationPort::latest_pair(&e), accepted);
    let stale = GameplayPresentationPort::validate_host(&e, point(1, 3_000_000_001)).unwrap_err();
    assert_eq!(
        stale.downcast_ref::<EstimatorError>(),
        Some(&EstimatorError::Stale)
    );
    let mut transport = Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::ZERO);
    let before = transport.clone();
    let error = GameplayPresentationPort::update(&mut e, point(1, 1_000_000_000), &mut transport)
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<EstimatorError>(),
        Some(&EstimatorError::NonpositiveTransport)
    );
    assert_eq!(transport, before);
    assert_eq!(GameplayPresentationPort::latest_pair(&e), accepted);
}
#[test]
fn fresh_port_constructor_and_memory_reseed_use_distinct_playback_origin() {
    let mut e = <PresentationEstimator as GameplayPresentationPort>::new_with_playback_origin(
        DisciplineConfig::default(),
        point(2, -10_000_000_000),
        point(2, -8_000_000_000),
        ClockDomainId(1),
        Timestamp::from_nanos(-500_000_000),
    )
    .unwrap();
    assert_eq!(GameplayPresentationPort::latest_pair(&e), None);
    let (mut device, _producer) = device(false, vec![]);
    let original = pair(-7_000_000_000, -2_000_000_000);
    GameplayDevice::seed_resume(&mut device, &mut e, original).unwrap();
    e.observe_clock_pair(pair(-6_000_000_000, -1_000_000_000))
        .unwrap();
    let mut transport = Transport::new(
        Timestamp::from_nanos(-3_000_000_000),
        Timestamp::from_nanos(-500_000_000),
        Rate::NORMAL,
    );
    assert_eq!(
        GameplayPresentationPort::update(&mut e, point(1, -1_000_000_000), &mut transport).unwrap(),
        DisciplineUpdate::Applied {
            base_rate_ppm: 0,
            correction_ppm: 0,
            applied_rate_ppm: 0,
            phase_error_ns: 0,
            limited: false
        }
    );
    assert_eq!(device.seeds, [original]);
    assert_eq!(
        transport.position_at(Timestamp::ZERO).unwrap(),
        Timestamp::from_nanos(2_500_000_000)
    );
}
