//! Actual solo service/entry and source Mixer; scripted IO grants no hardware accuracy.
use super::*;
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    bgm::BgmConfig,
    gameplay::output::ports::{
        NativeSourcePlaybackQuery, SourcePlaybackFacts, SourcePlaybackIdentity,
    },
    gameplay_competition::NoopSoloCompetition,
    gameplay_presentation::GameplayDevice,
    native_audio_presentation::{NativeAudioSnapshot, TargetNativeAudioSnapshot},
    native_audio_startup::new_coordinate_audio_presentation,
    native_output_coordinate::{
        CoordinateNativeAudioSnapshot, CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate,
    },
    native_playback_phase::{ConvertedPlaybackFacts, NativePlaybackMapping},
    play_policy::ResolvedPlayPolicy,
    replay_speed::ReplaySpeed,
};
use beatkernel::{
    audio::{
        command_queue, AudioFormat, AudioLimits, ConvertedPhaseSpan, InterpolatedTargetTime, Mixer,
        MixerConfig, OutputFrameBasis, PcmLimits, PcmSample, SampleBank, SampleId,
        TargetFrameBasis, TargetTime, VoiceId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId,
    },
    judge::{JudgeEngine, JudgeStage},
    replay::{ReplayOperation, codec::ReplayCodecLimits},
    runtime::{RuntimeProcessingClock, SoundBinding},
    time::{presentation::PresentationEstimator, ExtrapolationPolicy},
    transport::{OutputPhaseBasis, Rate, Transport},
};
use beatkernel_platform::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters, presentation::validation::OriginalNativePresentationEvidence,
};
use std::cell::Cell;

fn cp(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn source() -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(
        "#BPM 60\n#TOTAL 100\n#WAV01 key.wav\n#00011:01\n#00111:01",
        Default::default(),
    )
    .unwrap()
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn input(at: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), cp(1, at), sequence),
        control: PhysicalControlId::keyboard(7u16),
        state,
    })
}
struct Device {
    mixer: Mixer,
    basis: OutputFrameBasis,
    target: Option<TargetFrameBasis>,
    report: RenderReport,
    missing: Cell<bool>,
    wrong_epoch: Cell<bool>,
    suspended: bool,
    mapping_calls: Cell<usize>,
    report_fail: bool,
    step: usize,
    events: VecDeque<PhysicalInputEvent>,
}
impl Device {
    fn actual_schedule(&self) -> ClockPoint {
        TargetTime::from_frames(
            self.report.playback_start_frame + self.report.playback_frames as u64,
            3,
        )
        .unwrap()
        .point(self.basis.origin())
        .unwrap()
    }
}
impl GameplayDevice for Device {
    type Presentation = PresentationEstimator;
    fn observe(&mut self, _: &mut Self::Presentation) -> NativeGameplayResult<()> {
        panic!("no scalar estimator path")
    }
    fn observe_audio(&mut self, _: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        self.step += 1;
        Ok(())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        if self.report_fail {
            Err("render receipt fault".into())
        } else {
            Ok(Some(self.report))
        }
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(cp(1, 110))
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        events.append(&mut self.events);
        Ok(InputBatch {
            completed_through: Some(cp(1, 110)),
            backlog: false,
            closed: self.step >= 2,
        })
    }
    fn observe_end(
        &mut self,
        _: &mut NativeEnd,
        _: &Self::Presentation,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        panic!("no scalar end path")
    }
    fn seed_resume(
        &mut self,
        _: &mut Self::Presentation,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        panic!("no scalar resume path")
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        Ok(self.actual_schedule())
    }
    fn output_clock_suspended(&self) -> bool {
        self.suspended
    }
    fn playback_mapping_at(
        &self,
        _: &NativeAudioPresentation,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<NativePlaybackMapping>> {
        self.mapping_calls.set(self.mapping_calls.get() + 1);
        if self.missing.get() {
            return Ok(None);
        }
        let history = self.mixer.source_playback_history().unwrap();
        let epoch = if self.wrong_epoch.get() { 1 } else { 0 };
        if let Some(basis) = self.target {
            return Ok(Some(NativePlaybackMapping::converted(
                epoch,
                basis,
                identity,
                *query,
                ConvertedPlaybackFacts {
                    phase: ConvertedPhaseSpan::from_words([3, 1, 0, 1, 0, 1, 3, 0]).unwrap(),
                    segment_end: TargetTime::from_frames(4, 3).unwrap(),
                    generated_target_end: TargetTime::from_frames(4, 3).unwrap(),
                    submitted_target_end: TargetTime::from_frames(4, 3).unwrap(),
                    playback: history.span(0).unwrap(),
                    pulled_source_end: history.pulled_end_frame(),
                    revision: 2,
                },
            )?));
        }
        let playback = history.lookup_at_source_time(*query).map_err(|_| {
            std::io::Error::other("actual source history does not cover the exact playback query")
        })?;
        let packet = NativeSourcePlaybackQuery::new(
            epoch,
            self.basis,
            identity,
            *query,
            SourcePlaybackFacts {
                playback,
                pulled_source_end: history.pulled_end_frame(),
                submitted_source_end: 4,
                render: Some(self.report),
                revision: 2,
            },
            Some(()),
        )?;
        Ok(Some(NativePlaybackMapping::from_source(&packet)))
    }
}
#[derive(Default)]
struct Host {
    reports: Vec<RuntimeReport>,
    fail: bool,
}
impl NativeGameplayHost for Host {
    fn cancelled(&self) -> bool {
        false
    }
    fn pause_requested(&self) -> bool {
        false
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, _: PauseState) {}
    fn publish_section_end(&mut self, _: Timestamp) {}
    fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        self.reports.push(report.clone());
        if self.fail {
            Err("post-operation publication refusal".into())
        } else {
            Ok(())
        }
    }
    fn publish_local_reports(
        &mut self,
        _: &[crate::local_runtime::PlayerReport],
    ) -> NativeGameplayResult<()> {
        panic!("solo only")
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
}
#[derive(Default)]
struct Control {
    waits: usize,
}
impl NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        Ok(0)
    }
    fn checked_add(moment: u64, d: WallDuration) -> Option<u64> {
        moment.checked_add(u64::try_from(d.as_nanos()).ok()?)
    }
    fn wait(&mut self, _: WallDuration) -> NativeGameplayResult<()> {
        self.waits += 1;
        if self.waits > 5 {
            return Err("fixture production loop did not close".into());
        }
        Ok(())
    }
}
struct Fixture {
    device: Device,
    runtime: SoloRuntime,
    presentation: NativeAudioPresentation,
    gauge: BmsGauge,
    bgm: BgmFeeder,
    pause: NativePause,
    end: Option<NativeEnd>,
    completion: Option<SongCompletion>,
    capture: Option<LiveReplayCapture>,
    competition: Option<NoopSoloCompetition>,
    delivery: InputDeliveryTelemetry,
    pre: u64,
    merger: InputMerger,
    cfg: CoordinateGameplayConfig,
    speed: LiveSpeedSetup,
    host: Host,
    last_song: Timestamp,
    last_host: ClockPoint,
}
impl Fixture {
    fn new(target: bool, rate: Rate) -> Self {
        let format = AudioFormat::new(3, 1).unwrap();
        let pcm = PcmLimits::new(64, 256, 1).unwrap();
        let mut bank = SampleBank::new(format, pcm).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25; 8], pcm).unwrap(),
        )
        .unwrap();
        let (producer, consumer) = command_queue(16).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(2),
                Timestamp::ZERO,
                AudioLimits::new(16, 8, 16, 16, 16).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        mixer.enable_source_playback_history(8).unwrap();
        mixer.render(&mut [0.0; 1]).unwrap();
        let basis = mixer.output_frame_basis();
        let report = mixer.render(&mut [0.0; 3]).unwrap();
        let target_basis = target.then(|| {
            TargetFrameBasis::new(
                cp(2, 0),
                TargetTime::from_frames(1, 3).unwrap(),
                1_000_000_000,
            )
            .unwrap()
        });
        let descriptor = target_basis.map_or(
            AudioAuthorityCoordinateBasis::SourceCounter {
                basis,
                frequency: 10_000_000_000,
            },
            AudioAuthorityCoordinateBasis::TargetFrame,
        );
        let mut presentation = new_coordinate_audio_presentation(
            0,
            descriptor,
            ClockDomainId(1),
            cp(3, 500),
            AudioAuthorityConfig {
                input_extrapolation: ExtrapolationPolicy::Bounded {
                    before: Duration::from_nanos(2),
                    after: Duration::ZERO,
                },
                ..AudioAuthorityConfig::default()
            },
        )
        .unwrap();
        for (n, at) in [
            (if target { 0 } else { 1 }, 100),
            (if target { 1 } else { 21 }, 110),
        ] {
            if let Some(basis) = target_basis {
                presentation
                    .admit_target_coordinate(
                        CoordinateTargetNativeAudioSnapshot::new(
                            TargetNativeAudioSnapshot {
                                epoch: 0,
                                basis,
                                evidence: OriginalNativePresentationEvidence::SuppliedPair(
                                    ClockPair {
                                        source: basis.point_at_stream_frame(n).unwrap(),
                                        target: cp(1, at),
                                    },
                                ),
                            },
                            NativeOutputCoordinate::StreamFrame { frame: n },
                        )
                        .unwrap(),
                    )
                    .unwrap();
            } else {
                presentation
                    .admit_coordinate(
                        CoordinateNativeAudioSnapshot::new(
                            NativeAudioSnapshot {
                                epoch: 0,
                                basis,
                                evidence: OriginalNativePresentationEvidence::Wasapi {
                                    basis: Some(basis),
                                    snapshot: AudioStreamSnapshot {
                                        telemetry_available: true,
                                        status: AudioStreamStatus::Running,
                                        counters: StreamCounters::default(),
                                        render: None,
                                        clock: Some(AudioClockSnapshot {
                                            position: n,
                                            frequency: 10_000_000_000,
                                            qpc_100ns: n,
                                            reading_quality: AudioClockReadingQuality::Accurate,
                                            host_point: Some(cp(1, at)),
                                            mapping_quality: ClockMappingQuality::Unknown,
                                        }),
                                    },
                                },
                            },
                            NativeOutputCoordinate::Counter {
                                position: n,
                                frequency: 10_000_000_000,
                            },
                        )
                        .unwrap(),
                    )
                    .unwrap();
            }
        }
        let speed = LiveSpeedSetup::new(
            Timestamp::ZERO,
            Duration::ZERO,
            ReplaySpeed::new(rate).unwrap(),
            None,
        )
        .unwrap();
        let phase = OutputPhaseBasis::new(ClockDomainId(3), 91, 13, 1, 3).unwrap();
        let transport = Transport::new_with_output_phase(
            phase,
            cp(3, 500),
            Timestamp::ZERO,
            Duration::ZERO,
            rate,
            None,
        )
        .unwrap();
        let chart = source();
        let policy = ResolvedPlayPolicy::builtin(0, 0, 0).unwrap();
        let judge = JudgeEngine::new(
            chart.compile().unwrap().chart,
            chart.rules(),
            policy.judge().clone(),
        )
        .unwrap();
        let capture = LiveReplayCapture::new_with_policy_and_speed(
            &judge,
            ClockDomainId(3),
            limits(),
            Timestamp::ZERO,
            0,
            None,
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            &policy,
            speed.speed(),
        )
        .unwrap();
        let sounds = judge
            .chart()
            .objects()
            .iter()
            .map(|o| SoundBinding {
                object: o.id,
                stage: JudgeStage::Instant,
                sample: SampleId(1),
                voice: VoiceId(o.id.0 + 1),
                gain: 1.0,
            })
            .collect();
        let bindings = BindingMap::from_bindings([Binding {
            device: DeviceSelector::Any,
            physical: PhysicalControlId::keyboard(7u16),
            game_control: GameControlId(0x11),
        }])
        .unwrap();
        let mut runtime = SoloRuntime::new(
            ClockDomainId(3),
            ClockDomainId(2),
            transport,
            bindings,
            judge,
            producer,
            sounds,
            16,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        let stream = presentation.authority().epoch().stream_origin;
        let cfg = CoordinateGameplayConfig {
            audio: AudioGameplayConfig {
                gameplay: NativeGameplayConfig {
                    origin: cp(1, 90),
                    stream_origin: stream,
                    playback_origin: stream,
                    song_origin: Timestamp::ZERO,
                    sample_rate: 3,
                    end_song: None,
                    advance_lag: Duration::ZERO,
                    seconds: None,
                    pause_supported: false,
                    logical_schedule: true,
                },
                section_start: Timestamp::ZERO,
            },
            source_history: SourcePlaybackIdentity::new(3, 0, 0).unwrap(),
        };
        let merger = InputMerger::new_dynamic(ClockDomainId(1), cp(1, 90), 4096, 16).unwrap();
        Self {
            device: Device {
                mixer,
                basis,
                target: target_basis,
                report,
                missing: Cell::new(false),
                wrong_epoch: Cell::new(false),
                suspended: false,
                mapping_calls: Cell::new(0),
                report_fail: false,
                step: 0,
                events: VecDeque::new(),
            },
            runtime,
            presentation,
            gauge: BmsGauge::new(policy.gauge().try_copy().unwrap()),
            bgm: BgmFeeder::new(
                vec![],
                BgmConfig {
                    output_origin: cp(2, 0),
                    sample_rate: 3,
                    preroll: Duration::ZERO,
                    lookahead: Duration::from_nanos(1),
                    max_pending: 1,
                },
            )
            .unwrap(),
            pause: NativePause::new(cp(2, 0), ClockDomainId(1), 3).unwrap(),
            end: None,
            completion: None,
            capture: Some(capture),
            competition: None,
            delivery: InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap(),
            pre: 0,
            merger,
            cfg,
            speed,
            host: Host::default(),
            last_song: Timestamp::ZERO,
            last_host: cp(1, 90),
        }
    }
    fn admit(&mut self, at: i64, sequence: u64, state: ButtonState) {
        self.merger.register_source(DeviceId(9)).unwrap();
        self.merger
            .admit(input(at, sequence, state), cp(1, 110))
            .unwrap();
        self.presentation
            .authority_mut()
            .record_acquired_prefix(cp(1, 110))
            .unwrap();
    }
    fn service(
        &mut self,
        now: i64,
        input_only: bool,
        practice_end: Option<Timestamp>,
    ) -> NativeGameplayResult<()> {
        let mut timing = AudioTiming(
            &mut self.presentation,
            Timestamp::ZERO,
            Some(self.speed),
            Some(self.cfg.source_history),
        );
        let mut session = GameplaySession {
            runtime: &mut self.runtime,
            gauge: &mut self.gauge,
            bgm: &mut self.bgm,
            discipline: &mut timing,
            pause: &mut self.pause,
            end: &mut self.end,
            completion: &mut self.completion,
            capture: &mut self.capture,
            competition: &mut self.competition,
            delivery: &mut self.delivery,
            pre_origin_inputs: &mut self.pre,
        };
        service_audio_solo(
            &mut self.device,
            &mut session,
            &mut self.cfg.audio.gameplay,
            &mut self.merger,
            cp(1, now),
            false,
            &mut PauseKeyboard::new(),
            &mut None,
            &mut None,
            &mut false,
            &mut self.last_song,
            &mut self.last_host,
            false,
            None,
            &mut OwnedStopEvidence::default(),
            &mut self.host,
            input_only.then_some(cp(1, 200)),
            None,
            practice_end,
            &mut NativeRenderEvidence::default(),
            None,
            &mut None,
            &mut false,
        )
    }
    fn run(&mut self) -> NativeGameplayResult<Option<CompletedPlayResult>> {
        self.run_with_speed(Some(self.speed))
    }
    fn run_with_speed(
        &mut self,
        speed: Option<LiveSpeedSetup>,
    ) -> NativeGameplayResult<Option<CompletedPlayResult>> {
        self.cfg.audio.gameplay.logical_schedule = false;
        run_gameplay_audio_coordinate_with_result_and_ports(
            &mut self.device,
            AudioGameplaySession {
                session: GameplaySession {
                    runtime: &mut self.runtime,
                    gauge: &mut self.gauge,
                    bgm: &mut self.bgm,
                    discipline: &mut self.presentation,
                    pause: &mut self.pause,
                    end: &mut self.end,
                    completion: &mut self.completion,
                    capture: &mut self.capture,
                    competition: &mut self.competition,
                    delivery: &mut self.delivery,
                    pre_origin_inputs: &mut self.pre,
                },
                merger: &mut self.merger,
            },
            self.cfg,
            &mut Control::default(),
            &mut self.host,
            speed,
        )
    }
}

#[test]
fn same_floored_original_counter_inputs_use_exact_source_cut_and_raw_render_schedule() {
    for rate in [
        Rate::NORMAL,
        Rate::new(1, 2).unwrap(),
        Rate::new(3, 2).unwrap(),
        Rate::new(2, 1).unwrap(),
    ] {
        let mut f = Fixture::new(false, rate);
        f.admit(103, 0, ButtonState::Down);
        f.admit(106, 1, ButtonState::Up);
        f.service(110, true, None).unwrap();
        assert_eq!(f.host.reports.len(), 2);
        assert_eq!(f.host.reports[0].song_time, Timestamp::ZERO);
        assert_eq!(
            f.host.reports[1].song_time,
            Timestamp::from_nanos(rate.numerator() / rate.denominator() as i64)
        );
        for (index, r) in f.host.reports.iter().enumerate() {
            assert_eq!(
                r.input.as_ref().unwrap().meta().timestamp,
                Timestamp::from_nanos(501)
            );
            assert_eq!(
                r.input.as_ref().unwrap().meta().original_clock_point,
                Some(cp(1, if index == 0 { 103 } else { 106 }))
            );
            assert_eq!(r.audio_at, f.device.actual_schedule());
            assert_eq!(r.input_mapping_quality, ClockMappingQuality::Unknown);
        }
        assert_eq!(f.merger.pending(), 0);
        assert_eq!(
            f.presentation.authority().committed_input_host(),
            Some(cp(1, 106))
        );
        assert_eq!(f.last_host, cp(1, 106));
    }
}
#[test]
fn target_hz_differs_from_source_and_samefloor_inputs_still_use_original_active_phase() {
    let mut f = Fixture::new(true, Rate::new(2, 1).unwrap());
    f.admit(107, 0, ButtonState::Down);
    f.admit(110, 1, ButtonState::Up);
    f.service(110, true, None).unwrap();
    assert_eq!(f.host.reports.len(), 2);
    assert_eq!(f.host.reports[0].song_time, Timestamp::ZERO);
    assert_eq!(f.host.reports[1].song_time, Timestamp::from_nanos(2));
    assert_eq!(
        f.host.reports[0].input.as_ref().unwrap().meta().timestamp,
        f.host.reports[1].input.as_ref().unwrap().meta().timestamp
    );
    assert_eq!(
        f.presentation.target_basis().unwrap().sample_rate(),
        1_000_000_000
    );
}
#[test]
fn unavailable_mapping_holds_the_original_event_without_scalar_fallback() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.admit(103, 0, ButtonState::Down);
    f.device.missing.set(true);
    let before = format!("{:?}", f.presentation);
    let event = f.merger.peek_ready(cp(1, 110)).unwrap().cloned();
    f.service(110, true, None).unwrap();
    assert_eq!(f.merger.peek_ready(cp(1, 110)).unwrap(), event.as_ref());
    assert_eq!(format!("{:?}", f.presentation), before);
    assert!(f.host.reports.is_empty());
    assert!(f.runtime.judge().effective_song_time().is_none());
    f.device.missing.set(false);
    f.service(110, true, None).unwrap();
    assert_eq!(f.host.reports.len(), 1);
}
#[test]
fn suspension_and_expired_evidence_hold_input_and_watermarks() {
    for suspended in [false, true] {
        let mut f = Fixture::new(false, Rate::NORMAL);
        f.admit(103, 0, ButtonState::Down);
        f.device.suspended = suspended;
        let before = format!("{:?}", f.presentation);
        f.service(if suspended { 110 } else { 1_000_000_111 }, true, None)
            .unwrap();
        assert_eq!(f.merger.pending(), 1);
        assert!(f.host.reports.is_empty());
        assert_eq!(format!("{:?}", f.presentation), before);
    }
}
#[test]
fn true_preorigin_input_is_retired_without_query_or_runtime_operation() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.admit(98, 0, ButtonState::Down);
    f.service(110, true, None).unwrap();
    assert_eq!(f.pre, 1);
    assert_eq!(f.merger.pending(), 0);
    assert_eq!(f.device.mapping_calls.get(), 0);
    assert!(f.host.reports.is_empty());
    assert_eq!(
        f.presentation.authority().committed_input_host(),
        Some(cp(1, 98))
    );
    assert_eq!(f.presentation.authority().committed_operation(), None);
    assert!(f.capture.as_ref().unwrap().records().is_empty());
}
#[test]
fn finite_equal_phase_endpoint_holds_original_input_until_real_boundary() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.admit(106, 0, ButtonState::Down);
    let before = format!("{:?}", f.presentation);
    f.service(110, true, Some(Timestamp::from_nanos(1)))
        .unwrap();
    assert_eq!(f.merger.pending(), 1);
    assert!(f.host.reports.is_empty());
    assert_eq!(format!("{:?}", f.presentation), before);
}
#[test]
fn wrong_mapping_identity_and_render_schedule_error_refuse_before_input_pop() {
    for wrong_mapping in [false, true] {
        let mut f = Fixture::new(false, Rate::NORMAL);
        f.admit(103, 0, ButtonState::Down);
        f.device.wrong_epoch.set(wrong_mapping);
        f.device.report_fail = !wrong_mapping;
        let before = format!("{:?}", f.presentation);
        assert!(f.service(110, true, None).is_err());
        assert_eq!(f.merger.pending(), 1);
        assert!(f.host.reports.is_empty());
        assert_eq!(format!("{:?}", f.presentation), before);
    }
}
#[test]
fn real_observed_frontier_uses_retained_original_coordinate_and_commits_once() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.presentation
        .authority_mut()
        .record_acquired_prefix(cp(1, 110))
        .unwrap();
    f.service(120, false, None).unwrap();
    assert_eq!(f.host.reports.len(), 1);
    assert_eq!(f.host.reports[0].song_time, Timestamp::from_nanos(2));
    assert_eq!(
        f.presentation.authority().closed_host_prefix(),
        Some(cp(1, 110))
    );
    assert_eq!(f.last_host, cp(1, 110));
    f.service(120, false, None).unwrap();
    assert_eq!(f.host.reports.len(), 1);
}
#[test]
fn missing_frontier_mapping_cannot_close_acquired_prefix() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.presentation
        .authority_mut()
        .record_acquired_prefix(cp(1, 110))
        .unwrap();
    f.device.missing.set(true);
    let before = format!("{:?}", f.presentation);
    f.service(110, false, None).unwrap();
    assert_eq!(format!("{:?}", f.presentation), before);
    assert!(f.host.reports.is_empty());
}
#[test]
fn genuine_runtime_commit_survives_fallible_report_observer_and_preserves_capture() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.admit(103, 0, ButtonState::Down);
    f.host.fail = true;
    let error = f.service(110, true, None).unwrap_err();
    assert!(error
        .downcast_ref::<NativeReportObservationError>()
        .is_some());
    assert_eq!(f.merger.pending(), 0);
    assert_eq!(
        f.presentation.authority().committed_input_host(),
        Some(cp(1, 103))
    );
    assert_eq!(f.host.reports.len(), 1);
    assert!(!f.capture.as_ref().unwrap().records().is_empty());
}
#[test]
fn existing_raw_keysound_timestamp_executes_on_actual_source_mixer() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.admit(103, 0, ButtonState::Down);
    f.service(110, true, None).unwrap();
    assert!(!f.host.reports[0].audio_commands.is_empty());
    let mut pcm = [0.0; 1];
    f.device.mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25]);
}
#[test]
fn real_coordinate_entry_preserves_canonical_capture_codec_and_rejudge() {
    let mut f = Fixture::new(false, Rate::new(2, 1).unwrap());
    f.device.events.push_back(input(103, 0, ButtonState::Down));
    f.device.events.push_back(input(106, 1, ButtonState::Up));
    assert!(f.run().unwrap().is_none());
    assert_eq!(f.device.step, 2);
    let file = f.capture.take().unwrap().into_file();
    let encoded = beatkernel::replay::codec::encode_replay(&file, limits()).unwrap();
    let decoded = beatkernel::replay::codec::decode_replay(&encoded, limits()).unwrap();
    assert_eq!(decoded, file);
    let records = decoded
        .records
        .iter()
        .filter(|r| matches!(r.operation, ReplayOperation::Input(_)))
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].song_time, Timestamp::ZERO);
    assert_eq!(records[1].song_time, Timestamp::from_nanos(2));
    let results = f
        .host
        .reports
        .iter()
        .flat_map(|r| r.judge_events.iter().copied())
        .collect::<Vec<_>>();
    let rejudged =
        crate::replay_playback::reconstruct_section(&source(), decoded, limits()).unwrap();
    assert_eq!(rejudged.results(), results);
    assert_eq!(
        rejudged.engine().stable_hash().unwrap(),
        f.runtime.judge().stable_hash().unwrap()
    );
}
#[test]
fn coordinate_entry_cold_refusal_has_no_device_or_judge_side_effect() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.cfg.source_history = SourcePlaybackIdentity::new(4, 0, 0).unwrap();
    let before = format!("{:?}", f.presentation);
    assert!(f.run().is_err());
    assert_eq!(f.device.step, 0);
    assert!(f.host.reports.is_empty());
    assert!(f.runtime.judge().effective_song_time().is_none());
    assert_eq!(format!("{:?}", f.presentation), before);
}

#[test]
fn preorigin_counter_exhaustion_refuses_before_event_or_authority_mutation() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.admit(98, 0, ButtonState::Down);
    f.pre = u64::MAX;
    let before = format!("{:?}", f.presentation);
    assert!(f.service(110, true, None).is_err());
    assert_eq!(f.pre, u64::MAX);
    assert_eq!(f.merger.pending(), 1);
    assert_eq!(format!("{:?}", f.presentation), before);
    assert!(f.host.reports.is_empty());
}

#[test]
fn finite_observed_frontier_equal_to_phase_endpoint_cannot_close_or_advance() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.presentation
        .authority_mut()
        .record_acquired_prefix(cp(1, 110))
        .unwrap();
    let before = format!("{:?}", f.presentation);
    f.service(110, false, Some(Timestamp::from_nanos(2)))
        .unwrap();
    assert_eq!(format!("{:?}", f.presentation), before);
    assert!(f.host.reports.is_empty());
    assert!(f.presentation.authority().closed_host_prefix().is_none());
}

#[test]
fn normal_coordinate_entry_without_selected_speed_accepts_matching_real_owners() {
    let mut f = Fixture::new(false, Rate::NORMAL);
    f.device.events.push_back(input(103, 0, ButtonState::Down));
    assert!(f.run_with_speed(None).unwrap().is_none());
    assert_eq!(f.device.step, 2);
    assert!(f.host.reports.iter().any(|report| report.input.is_some()));
    assert_eq!(
        f.presentation.authority().committed_input_host(),
        Some(cp(1, 103))
    );
}

#[test]
fn normal_coordinate_entry_rejects_wrong_pause_epoch_host_phase_and_actual_runtime_end() {
    for mismatch in 0..4 {
        let mut f = Fixture::new(false, Rate::NORMAL);
        match mismatch {
            0 => {
                // The public cold constructor supplies an independent epoch;
                // this does not claim a target stream or a pause acknowledgement.
                f.pause = NativePause::new(cp(2, 0), ClockDomainId(1), 3)
                    .unwrap()
                    .with_target_basis(
                        1,
                        TargetFrameBasis::new(cp(2, 0), TargetTime::from_frames(0, 3).unwrap(), 3)
                            .unwrap(),
                    )
                    .unwrap();
                assert_ne!(f.pause.epoch(), f.presentation.authority().epoch().id);
            }
            1 => f.pause = NativePause::new(cp(2, 0), ClockDomainId(9), 3).unwrap(),
            2 => {
                assert!(f
                    .pause
                    .request(true, f.presentation.latest_record().unwrap().pair())
                    .unwrap());
                assert_eq!(f.pause.phase(), PausePhase::Pausing);
            }
            _ => f.runtime.set_song_end(Timestamp::from_nanos(1)).unwrap(),
        }
        // Mathematical configuration/Transport coherence alone still passes.
        crate::native_policy_admission::validate_audio_coordinate_start(
            &f.cfg,
            &f.presentation,
            f.runtime.transport_mut(),
            None,
        )
        .unwrap();
        let original = input(103, 0, ButtonState::Down);
        f.device.events.push_back(original.clone());
        let authority = format!("{:?}", f.presentation);
        let transport = f.runtime.transport_mut().clone();
        let judge = f.runtime.judge().stable_hash().unwrap();
        let pause = format!("{:?}", f.pause);
        let source_cursor = f.device.mixer.frame_cursor();
        let runtime_end = f.runtime.song_end();
        assert!(f.run_with_speed(None).is_err(), "mismatch={mismatch}");
        assert_eq!(f.device.step, 0);
        assert_eq!(f.device.events.front(), Some(&original));
        assert_eq!(f.device.mapping_calls.get(), 0);
        assert_eq!(format!("{:?}", f.presentation), authority);
        assert_eq!(&*f.runtime.transport_mut(), &transport);
        assert_eq!(f.runtime.judge().stable_hash().unwrap(), judge);
        assert_eq!(f.runtime.judge().effective_song_time(), None);
        assert_eq!(f.runtime.song_end(), runtime_end);
        assert_eq!(format!("{:?}", f.pause), pause);
        assert_eq!(f.device.mixer.frame_cursor(), source_cursor);
        assert!(f.host.reports.is_empty());
        assert!(f.capture.as_ref().unwrap().records().is_empty());
    }
}
