//! Public native entrypoints accept either a direct common port or a legacy adapter.
//! Original events, clock observations and real mixer drains remain unchanged.
#![cfg(not(target_arch = "wasm32"))]

use beatkernel::{
    audio::{
        command_queue, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample,
        RenderReport, SampleBank, SampleId, VoiceId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow},
    runtime::SoundBinding,
    telemetry::InputDeliveryTelemetry,
    time::{
        presentation::DisciplineConfig, ClockDomainId, ClockPair, ClockPoint, Duration, Timestamp,
    },
    transport::{Rate, Transport},
};
use beatkernel_bms_runtime::{
    bgm::{BgmConfig, BgmFeeder},
    competition_live::LiveCompetition,
    completion::SongCompletion,
    gameplay_presentation::GameplayDevice,
    gauge::BmsGauge,
    local_runtime::SoloRuntime,
    native_end::{EndBoundary, NativeEnd},
    native_gameplay::{
        retain_input, run_gameplay, run_gameplay_with_control, InputBatch, NativeGameplayConfig,
        NativeGameplayDevice, NativeGameplayResult, NativeGameplaySession,
    },
    native_pump_control::NativePumpControl,
    playback_pause::NativePause,
    replay_capture::LiveReplayCapture,
};
use beatkernel_platform::audio::presentation::discipline::PresentationDiscipline;
use std::{collections::VecDeque, time::Duration as WallDuration};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    None,
    Observe,
    Acquire,
    Host,
    Schedule,
    UncoveredInput,
}
struct DeviceState {
    mixer: Mixer,
    report: Option<RenderReport>,
    step: usize,
    pcm: Vec<f32>,
    calls: Vec<&'static str>,
    failure: Failure,
}
impl DeviceState {
    fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
        self.calls.push("observe");
        if self.failure == Failure::Observe {
            return Err("original device observation refused".into());
        }
        let mut pcm = [0.0; 10];
        self.report = Some(self.mixer.render(&mut pcm)?);
        self.pcm.extend_from_slice(&pcm);
        self.step += 1;
        discipline.observe_clock_pair(ClockPair {
            source: point(2, self.step as i64 * 10_000_000),
            target: point(1, self.step as i64 * 10_000_000),
        })?;
        Ok(())
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        self.calls.push("acquire");
        if self.failure == Failure::Acquire {
            return Err("original acquisition refused".into());
        }
        if self.step == 2 {
            retain_input(
                events,
                PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(
                        DeviceId(1),
                        point(
                            1,
                            if self.failure == Failure::UncoveredInput {
                                30_000_000
                            } else {
                                20_000_000
                            },
                        ),
                        1,
                    ),
                    control: PhysicalControlId::keyboard(4),
                    state: ButtonState::Down,
                }),
            )?;
        }
        Ok(InputBatch {
            completed_through: Some(point(1, self.step as i64 * 10_000_000)),
            backlog: self.step == 2,
            closed: self.step >= 4,
        })
    }
}
struct CanonicalDevice(DeviceState);
struct LegacyDevice(DeviceState);
// These implementations deliberately differ only in the public contract used.
// No reverse adapter or alternate gameplay implementation exists in the fixture.
macro_rules! device_methods {
    () => {
        fn observe(&mut self, p: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
            self.0.observe(p)
        }
        fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
            self.0.calls.push("report");
            Ok(self.0.report)
        }
        fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
            if self.0.failure == Failure::Host {
                return Err("original host observation refused".into());
            }
            Ok(point(1, self.0.step as i64 * 10_000_000))
        }
        fn acquire(
            &mut self,
            events: &mut VecDeque<PhysicalInputEvent>,
        ) -> NativeGameplayResult<InputBatch> {
            self.0.acquire(events)
        }
        fn observe_end(
            &mut self,
            end: &mut NativeEnd,
            p: &PresentationDiscipline,
            report: Option<RenderReport>,
        ) -> NativeGameplayResult<Option<EndBoundary>> {
            Ok(end.observe(report, p.latest_pair().ok_or("no original clock pair")?)?)
        }
        fn seed_resume(
            &mut self,
            p: &mut PresentationDiscipline,
            reference: ClockPair,
        ) -> NativeGameplayResult<()> {
            self.0.calls.push("resume");
            p.observe_clock_pair(reference)?;
            Ok(())
        }
        fn fallback_schedule(&mut self, rate: u32) -> NativeGameplayResult<ClockPoint> {
            self.0.calls.push("schedule");
            assert_eq!(rate, 1000);
            if self.0.failure == Failure::Schedule {
                return Err("original scheduling refused".into());
            }
            Ok(point(2, self.0.step as i64 * 10_000_000))
        }
    };
}
impl GameplayDevice for CanonicalDevice {
    type Presentation = PresentationDiscipline;
    device_methods!();
}
impl NativeGameplayDevice for LegacyDevice {
    device_methods!();
}

#[derive(Default)]
struct Control {
    waits: usize,
}
impl NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        Ok(self.waits as u64)
    }
    fn checked_add(at: u64, duration: WallDuration) -> Option<u64> {
        at.checked_add(duration.as_secs())
    }
    fn wait(&mut self, _: WallDuration) -> NativeGameplayResult<()> {
        self.waits += 1;
        Ok(())
    }
}
struct Session {
    runtime: SoloRuntime,
    gauge: BmsGauge,
    bgm: BgmFeeder,
    discipline: PresentationDiscipline,
    pause: NativePause,
    end: Option<NativeEnd>,
    completion: Option<SongCompletion>,
    capture: Option<LiveReplayCapture>,
    competition: Option<LiveCompetition>,
    delivery: InputDeliveryTelemetry,
    pre_origin_inputs: u64,
}
impl Session {
    fn new(failure: Failure) -> (Self, DeviceState) {
        let source = beatkernel_bms::parse(
            "#BPM 3000\n#WAV01 original.wav\n#00011:00010000\n",
            Default::default(),
        )
        .unwrap();
        let compiled = source.compile().unwrap();
        let judge = JudgeEngine::new(
            compiled.chart.clone(),
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
        .unwrap();
        let bindings = BindingMap::from_bindings([Binding {
            device: DeviceSelector::Any,
            physical: PhysicalControlId::keyboard(4),
            game_control: GameControlId(0x11),
        }])
        .unwrap();
        let format = AudioFormat::new(1000, 1).unwrap();
        let limits = PcmLimits::new(64, 256, 1).unwrap();
        let mut bank = SampleBank::new(format, limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25, 0.5], limits).unwrap(),
        )
        .unwrap();
        let (producer, consumer) = command_queue(8).unwrap();
        let mixer = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(2),
                Timestamp::ZERO,
                AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        let runtime = SoloRuntime::new(
            ClockDomainId(1),
            ClockDomainId(2),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            bindings,
            judge,
            producer,
            vec![SoundBinding {
                object: compiled.chart.objects()[0].id,
                stage: JudgeStage::Instant,
                sample: SampleId(1),
                voice: VoiceId(1),
                gain: 1.0,
            }],
            8,
        )
        .unwrap();
        let mut discipline = PresentationDiscipline::new(
            DisciplineConfig::default(),
            point(2, 0),
            ClockDomainId(1),
            Timestamp::ZERO,
        )
        .unwrap();
        discipline
            .observe_clock_pair(ClockPair {
                source: point(2, 0),
                target: point(1, 0),
            })
            .unwrap();
        let bgm = BgmFeeder::new(
            vec![],
            BgmConfig {
                output_origin: point(2, 0),
                sample_rate: 1000,
                preroll: Duration::ZERO,
                lookahead: Duration::from_nanos(1_000_000_000),
                max_pending: 4,
            },
        )
        .unwrap();
        (
            Self {
                runtime,
                gauge: BmsGauge::default(),
                bgm,
                discipline,
                pause: NativePause::new(point(2, 0), ClockDomainId(1), 1000).unwrap(),
                end: None,
                completion: None,
                capture: None,
                competition: None,
                delivery: InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap(),
                pre_origin_inputs: 0,
            },
            DeviceState {
                mixer,
                report: None,
                step: 0,
                pcm: vec![],
                calls: vec![],
                failure,
            },
        )
    }
    fn borrowed(&mut self) -> NativeGameplaySession<'_> {
        NativeGameplaySession {
            runtime: &mut self.runtime,
            gauge: &mut self.gauge,
            bgm: &mut self.bgm,
            discipline: &mut self.discipline,
            pause: &mut self.pause,
            end: &mut self.end,
            completion: &mut self.completion,
            capture: &mut self.capture,
            competition: &mut self.competition,
            delivery: &mut self.delivery,
            pre_origin_inputs: &mut self.pre_origin_inputs,
        }
    }
}
fn config(logical: bool) -> NativeGameplayConfig {
    NativeGameplayConfig {
        origin: point(1, 0),
        stream_origin: point(2, 0),
        playback_origin: point(2, 0),
        song_origin: Timestamp::ZERO,
        sample_rate: 1000,
        end_song: None,
        advance_lag: Duration::from_nanos(10_000_000),
        seconds: None,
        pause_supported: false,
        logical_schedule: logical,
    }
}

#[test]
fn public_native_wrapper_preserves_real_pcm_and_acquisition_for_both_device_contracts() {
    for logical in [false, true] {
        let (mut common_session, state) = Session::new(Failure::None);
        let mut common = CanonicalDevice(state);
        let (mut legacy_session, state) = Session::new(Failure::None);
        let mut legacy = LegacyDevice(state);
        let mut common_control = Control::default();
        let mut legacy_control = Control::default();
        run_gameplay_with_control(
            &mut common,
            common_session.borrowed(),
            config(logical),
            &mut common_control,
        )
        .unwrap();
        run_gameplay_with_control(
            &mut legacy,
            legacy_session.borrowed(),
            config(logical),
            &mut legacy_control,
        )
        .unwrap();
        assert_eq!(common.0.pcm, legacy.0.pcm);
        assert_eq!(&common.0.pcm[20..23], &[0.25, 0.5, 0.0]);
        assert_eq!(common.0.calls, legacy.0.calls);
        assert_eq!(common.0.step, 4);
        assert_eq!(common_control.waits, legacy_control.waits);
        assert_eq!(common_session.pre_origin_inputs, 0);
        assert_eq!(legacy_session.pre_origin_inputs, 0);
        assert!(
            common_session.completion.is_none(),
            "closed input is not song completion"
        );
        assert!(legacy_session.completion.is_none());
    }
}

#[test]
fn public_native_wrapper_preserves_original_device_failures_without_extra_calls() {
    for (failure, expected) in [
        (Failure::Observe, "original device observation refused"),
        (Failure::Acquire, "original acquisition refused"),
        (Failure::Host, "original host observation refused"),
        (Failure::Schedule, "original scheduling refused"),
    ] {
        let (mut common_session, state) = Session::new(failure);
        let mut common = CanonicalDevice(state);
        let (mut legacy_session, state) = Session::new(failure);
        let mut legacy = LegacyDevice(state);
        let common_error = run_gameplay_with_control(
            &mut common,
            common_session.borrowed(),
            config(false),
            &mut Control::default(),
        )
        .unwrap_err();
        let legacy_error = run_gameplay_with_control(
            &mut legacy,
            legacy_session.borrowed(),
            config(false),
            &mut Control::default(),
        )
        .unwrap_err();
        assert_eq!(common_error.to_string(), expected);
        assert_eq!(legacy_error.to_string(), expected);
        assert_eq!(common.0.calls, legacy.0.calls);
        assert_eq!(common.0.pcm, legacy.0.pcm);
    }
}

#[test]
fn canonical_and_legacy_devices_keep_original_input_frontier_refusal() {
    let (mut common_session, state) = Session::new(Failure::UncoveredInput);
    let mut common = CanonicalDevice(state);
    let (mut legacy_session, state) = Session::new(Failure::UncoveredInput);
    let mut legacy = LegacyDevice(state);
    let common_error = run_gameplay_with_control(
        &mut common,
        common_session.borrowed(),
        config(false),
        &mut Control::default(),
    )
    .unwrap_err();
    let legacy_error = run_gameplay_with_control(
        &mut legacy,
        legacy_session.borrowed(),
        config(false),
        &mut Control::default(),
    )
    .unwrap_err();
    assert_eq!(common_error.to_string(), legacy_error.to_string());
    assert_eq!(
        common_error.to_string(),
        "native input domain differs or timestamp is ahead of receipt"
    );
    assert_eq!(common.0.step, 2);
    assert_eq!(legacy.0.step, 2);
    assert_eq!(common.0.calls, legacy.0.calls);
    assert!(common_session.completion.is_none());
    assert!(legacy_session.completion.is_none());
}

#[test]
fn system_control_entrypoint_accepts_common_port_and_refuses_bad_config_before_io() {
    let (mut common_session, state) = Session::new(Failure::None);
    let mut common = CanonicalDevice(state);
    let (mut legacy_session, state) = Session::new(Failure::None);
    let mut legacy = LegacyDevice(state);
    let mut invalid = config(false);
    invalid.sample_rate = 0;
    let common_error = run_gameplay(&mut common, common_session.borrowed(), invalid).unwrap_err();
    let legacy_error = run_gameplay(&mut legacy, legacy_session.borrowed(), invalid).unwrap_err();
    assert_eq!(
        common_error.to_string(),
        "invalid native gameplay clocks/rate/lag"
    );
    assert_eq!(common_error.to_string(), legacy_error.to_string());
    assert!(common.0.calls.is_empty());
    assert!(legacy.0.calls.is_empty());
}
