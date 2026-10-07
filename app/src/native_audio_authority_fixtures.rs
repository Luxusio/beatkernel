//! Actual shared native audio pumps with scripted acquisition and a genuine Mixer.
use crate::{
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    bgm::{BgmConfig, BgmFeeder},
    competition::ScoreSummary,
    gameplay_competition::{NoopGroupCompetition, SoloCompetitionPort},
    gameplay_presentation::{GameplayAudioOutputContext, GameplayDevice, GameplayOutputContext},
    gauge::BmsGauge,
    live_pause::LivePauseObservation,
    local_input::InputMerger,
    local_players::PlayerId,
    local_runtime::{MemberConfig, PlayerReport, RuntimeGroup, SoloRuntime},
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
    native_cohort::{
        AudioCohortSession, GameplayPlayerState, run_cohort_audio_with_results_and_ports,
    },
    native_end::{EndBoundary, NativeEnd},
    native_gameplay::{
        AudioGameplaySession, GameplaySession, InputBatch, NativeGameplayConfig,
        NativeGameplayResult, run_gameplay_audio_with_policy_and_result_and_score_and_ports,
        run_gameplay_audio_with_result_and_ports,
    },
    native_gameplay_host::{
        NativeGameplayDiagnostic, NativeGameplayHost, NoopGameplayHost, PauseState,
    },
    native_pump_control::NativePumpControl,
    play_policy::{ClassifiedWindow, ResolvedPlayPolicy},
    play_result::CompletedPlayResult,
    playback_pause::NativePause,
    replay_capture::LiveReplayCapture,
};
use beatkernel::{
    audio::{
        AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample, RenderReport,
        SampleBank, SampleId, VoiceId, command_queue,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeOutcome, JudgeStage, JudgeWindow},
    replay::{ReplayHeader, ReplayOperation, codec::ReplayCodecLimits},
    runtime::{RuntimeProcessingClock, RuntimeReport, SoundBinding},
    telemetry::InputDeliveryTelemetry,
    time::{
        ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Duration, ExtrapolationPolicy,
        Timestamp, presentation::PresentationEstimator,
    },
    transport::{Rate, Transport},
};
use beatkernel_platform::audio::{
    asio::{AsioPresentationObservation, MultimediaHostInterval},
    presentation::validation::{NativePresentationValidator, OriginalNativePresentationEvidence},
};
use std::{cell::Cell, collections::VecDeque, rc::Rc};
const H: i64 = 10_000_000_000;
const R: i64 = 1_000_000_000;
const L: i64 = 5_000_000_000;
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    point(1, H + ns)
}
fn raw(ns: i64) -> ClockPoint {
    point(2, R + ns)
}
fn logical(ns: i64) -> ClockPoint {
    point(3, L + ns)
}
fn source(fanout: bool) -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(if fanout {
    "#BPM 3000\n#TOTAL 100\n#WAV01 key.wav\n#00011:00010000\n#00012:00010000\n#00013:00010000"}
    else {"#BPM 3000\n#TOTAL 100\n#WAV01 key.wav\n#00011:00010000\n#00112:01"},Default::default()).unwrap()
}
fn policy(source: &beatkernel_bms::BmsChart, classified: bool) -> ResolvedPlayPolicy {
    if classified {
        ResolvedPlayPolicy::bms(
            source,
            beatkernel_bms::BmsGaugeKind::Groove,
            &[ClassifiedWindow {
                judgment: beatkernel_bms::BmsJudgment::PGreat,
                window: JudgeWindow {
                    grade: JudgeGrade(91),
                    early: Duration::ZERO,
                    late: Duration::ZERO,
                },
            }],
            0,
        )
        .unwrap()
    } else {
        ResolvedPlayPolicy::builtin(0, 0, 0).unwrap()
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn native_presentation() -> NativeAudioPresentation {
    NativeAudioPresentation::new(
        AudioAuthority::new(
            AudioAuthorityConfig {
                history_capacity: 32,
                max_observation_age: Duration::from_nanos(1_000_000_000),
                input_extrapolation: ExtrapolationPolicy::Forbid,
                max_input_ahead: Duration::ZERO,
            },
            AudioAuthorityEpoch {
                id: 0,
                stream_origin: raw(0),
                logical_origin: logical(0),
                host_domain: ClockDomainId(1),
            },
        )
        .unwrap(),
        NativePresentationValidator::new(0, raw(0), ClockDomainId(1)),
    )
    .unwrap()
}
fn config(finite: bool, pause: bool) -> NativeGameplayConfig {
    NativeGameplayConfig {
        origin: host(0),
        stream_origin: raw(0),
        playback_origin: raw(0),
        song_origin: Timestamp::ZERO,
        sample_rate: 1000,
        end_song: finite.then_some(Timestamp::from_nanos(60_000_000)),
        advance_lag: Duration::from_nanos(10_000_000),
        seconds: None,
        pause_supported: pause,
        logical_schedule: true,
    }
}
fn bindings(device: Option<DeviceId>, fanout: bool) -> BindingMap {
    BindingMap::from_bindings((0..if fanout { 3 } else { 2 }).map(|i| Binding {
        device: device.map_or(DeviceSelector::Any, DeviceSelector::Exact),
        physical: PhysicalControlId::keyboard(if fanout { 7u16 } else { 7 + i }),
        game_control: GameControlId(0x11 + i as u32),
    }))
    .unwrap()
}
fn input(ns: i64, source: DeviceId, key: u16, seq: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(source, host(ns), seq),
        control: PhysicalControlId::keyboard(key),
        state,
    })
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Missing,
    OneAnchor,
    Stationary,
    Stale,
    PausePoint,
    PauseAsio,
    FutureAsio,
}
struct Device {
    mixer: Mixer,
    creation_basis: beatkernel::audio::OutputFrameBasis,
    report: Option<RenderReport>,
    step: usize,
    mode: Mode,
    cancel: Rc<Cell<bool>>,
    desired_pause: Rc<Cell<bool>>,
    sources: Vec<DeviceId>,
    finite: bool,
    backlog_three: bool,
    pcm: Vec<f32>,
    observed_pairs: Vec<ClockPair>,
}
impl Device {
    fn now(&self) -> ClockPoint {
        host(if self.mode == Mode::Stale && self.step >= 3 {
            2_000_000_001
        } else {
            self.step as i64 * 5_000_000
        })
    }
}
impl GameplayDevice for Device {
    type Presentation = PresentationEstimator;
    fn observe(&mut self, _: &mut Self::Presentation) -> NativeGameplayResult<()> {
        panic!("legacy estimator observe must not run")
    }
    fn pause_observation(&mut self, _: ClockPair) -> NativeGameplayResult<LivePauseObservation> {
        panic!("legacy point pause path must not replace original ASIO brackets")
    }
    fn publish_paused_output(
        &mut self,
        _: GameplayOutputContext<'_, Self::Presentation>,
    ) -> NativeGameplayResult<bool> {
        panic!("legacy paused-output timing context")
    }
    fn observe_audio(&mut self, p: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        self.step += 1;
        if self.step > 100 {
            return Err("audio pump exceeded fixture bound".into());
        }
        let mut pcm = [0.0; 10];
        let report = self.mixer.render(&mut pcm)?;
        self.report = Some(report);
        self.pcm.extend(pcm);
        let stop = if matches!(self.mode, Mode::PausePoint | Mode::PauseAsio) {
            18
        } else {
            7
        };
        if !self.finite && self.step >= stop {
            self.cancel.set(true);
        }
        if self.mode == Mode::Missing
            || (self.mode == Mode::OneAnchor && self.step > 1)
            || (self.mode == Mode::Stale && self.step > 2)
        {
            return Ok(());
        }
        let basis = self.creation_basis;
        let evidence = if matches!(self.mode, Mode::PauseAsio | Mode::FutureAsio) {
            let ns = report.start_frame as i64 * 500_000
                + if self.mode == Mode::FutureAsio && self.step >= 5 {
                    30_000_000
                } else {
                    0
                };
            let original = AsioPresentationObservation::from_render(
                report,
                1000,
                MultimediaHostInterval {
                    before: host(ns - 100_000),
                    after: host(ns + 100_000),
                },
                0,
                0,
                raw(0),
            )?;
            OriginalNativePresentationEvidence::Asio {
                observation: original,
                basis: Some(basis),
            }
        } else {
            let frames = if self.mode == Mode::Stationary && self.step > 2 {
                20
            } else {
                self.mixer.frame_cursor()
            };
            OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: raw(frames as i64 * 1_000_000),
                target: host(frames as i64 * 500_000),
            })
        };
        p.admit(NativeAudioSnapshot {
            epoch: 0,
            basis,
            evidence,
        })?;
        if let Some(record) = p.latest_record() {
            self.observed_pairs.push(record.pair());
        }
        Ok(())
    }
    fn audio_pause_observation(
        &mut self,
        p: &NativeAudioPresentation,
        now: ClockPoint,
    ) -> NativeGameplayResult<LivePauseObservation> {
        let record = p.latest_record().ok_or("fixture has no pause evidence")?;
        match *record.evidence() {
            OriginalNativePresentationEvidence::Asio { observation, .. } => Ok(
                crate::gameplay::output::adapters::observation::asio_pause_observation(
                    Some(observation),
                    now,
                ),
            ),
            _ => Ok(LivePauseObservation::Point(record.pair())),
        }
    }
    fn observe_audio_end(
        &mut self,
        end: &mut NativeEnd,
        p: &NativeAudioPresentation,
        rendered: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        let Some(record) = p.latest_record() else {
            return Ok(None);
        };
        Ok(match *record.evidence() {
            OriginalNativePresentationEvidence::Asio { observation, .. } => {
                end.observe_asio(observation)?
            }
            _ => end.observe(rendered, record.pair())?,
        })
    }
    fn publish_paused_audio_output(
        &mut self,
        _: GameplayAudioOutputContext<'_>,
        _: ClockPoint,
    ) -> NativeGameplayResult<bool> {
        Ok(false)
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(self.report)
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(self.now())
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        if matches!(self.mode, Mode::PausePoint | Mode::PauseAsio) {
            if self.step == 4 {
                self.desired_pause.set(true)
            }
            if self.step == 9 {
                self.desired_pause.set(false)
            }
        }
        if self.step == 2 {
            for &source in &self.sources {
                events.push_back(input(10_000_000, source, 7, 0, ButtonState::Down));
            }
        }
        if self.step == 3 {
            for &source in &self.sources {
                events.push_back(input(10_000_000, source, 7, 1, ButtonState::Up));
            }
        }
        if matches!(self.mode, Mode::PausePoint | Mode::PauseAsio) && self.step == 14 {
            for &source in &self.sources {
                events.push_back(input(65_000_000, source, 8, 2, ButtonState::Down));
            }
        }
        Ok(InputBatch {
            backlog: self.step == 2 || (self.backlog_three && self.step == 3),
            closed: false,
        })
    }
    fn observe_end(
        &mut self,
        _: &mut NativeEnd,
        _: &Self::Presentation,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        panic!("legacy end path")
    }
    fn seed_resume(
        &mut self,
        _: &mut Self::Presentation,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        panic!("legacy correlation reset")
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        Ok(raw(self.mixer.playback_frame_cursor() as i64 * 1_000_000))
    }
}
#[derive(Default)]
struct Control {
    waits: usize,
}
impl NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        Err("unbounded native fixture must not read HOST timer".into())
    }
    fn checked_add(now: u64, d: std::time::Duration) -> Option<u64> {
        now.checked_add(d.as_nanos() as u64)
    }
    fn wait(&mut self, _: std::time::Duration) -> NativeGameplayResult<()> {
        self.waits += 1;
        Ok(())
    }
}
struct Host {
    cancel: Rc<Cell<bool>>,
    desired: Rc<Cell<bool>>,
    reports: Vec<RuntimeReport>,
    local: Vec<Vec<PlayerReport>>,
    pause: Vec<PauseState>,
    completed: Vec<CompletedPlayResult>,
    refuse_hit: bool,
    prepared: Vec<PlayerId>,
}
impl NativeGameplayHost for Host {
    fn cancelled(&self) -> bool {
        self.cancel.get()
    }
    fn pause_requested(&self) -> bool {
        self.desired.get()
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, p: PauseState) {
        self.pause.push(p)
    }
    fn publish_section_end(&mut self, _: Timestamp) {}
    fn publish_report(&mut self, r: &RuntimeReport) -> NativeGameplayResult<()> {
        self.reports.push(r.clone());
        if self.refuse_hit
            && r.judge_events
                .iter()
                .any(|e| matches!(e.outcome, JudgeOutcome::Hit { .. }))
        {
            return Err("fixture observer refuses committed hit".into());
        }
        Ok(())
    }
    fn publish_local_reports(&mut self, r: &[PlayerReport]) -> NativeGameplayResult<()> {
        self.local.push(r.to_vec());
        Ok(())
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
    fn prepare_play_policies(
        &mut self,
        rows: &[(PlayerId, &ResolvedPlayPolicy)],
    ) -> NativeGameplayResult<()> {
        NoopGameplayHost.prepare_play_policies(rows)?;
        self.prepared = rows.iter().map(|r| r.0).collect();
        Ok(())
    }
    fn publish_completed_solo(&mut self, r: CompletedPlayResult) -> NativeGameplayResult<()> {
        self.completed.push(r);
        Ok(())
    }
}
struct Observer {
    header: ReplayHeader,
    times: Vec<Timestamp>,
    marks: usize,
}
impl SoloCompetitionPort for Observer {
    fn expected_policy_header(&self) -> Option<&ReplayHeader> {
        Some(&self.header)
    }
    fn observe(&mut self, r: &RuntimeReport) -> NativeGameplayResult<()> {
        self.times.push(r.song_time);
        Ok(())
    }
    fn mark_native_completed(&mut self) {
        self.marks += 1
    }
}
fn device(
    capacity: usize,
    finite: bool,
    mode: Mode,
    sources: Vec<DeviceId>,
) -> (Device, beatkernel::audio::CommandProducer, Host) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 8], pcm).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut chosen = MixerConfig::new(
        format,
        ClockDomainId(2),
        raw(0).timestamp,
        AudioLimits::new(capacity, 8, 16, 16, 16).unwrap(),
    );
    if finite {
        chosen = chosen.with_playback_end_frame(60)
    }
    let cancel = Rc::new(Cell::new(false));
    let desired = Rc::new(Cell::new(false));
    let host = Host {
        cancel: cancel.clone(),
        desired: desired.clone(),
        reports: vec![],
        local: vec![],
        pause: vec![],
        completed: vec![],
        refuse_hit: false,
        prepared: vec![],
    };
    let mixer = Mixer::new(chosen, bank, consumer).unwrap();
    // Native creation captures the original grid once; subsequent getter calls
    // return a new current-cursor basis rather than this stream's creation basis.
    let creation_basis = mixer.output_frame_basis();
    (
        Device {
            mixer,
            creation_basis,
            report: None,
            step: 0,
            mode,
            cancel,
            desired_pause: desired,
            sources,
            finite,
            backlog_three: false,
            pcm: vec![],
            observed_pairs: vec![],
        },
        producer,
        host,
    )
}
fn sounds(source: &beatkernel_bms::BmsChart, offset: u64) -> Vec<SoundBinding> {
    source
        .compile()
        .unwrap()
        .chart
        .objects()
        .iter()
        .map(|object| SoundBinding {
            object: object.id,
            stage: JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(object.id.0 + offset),
            gain: 1.0,
        })
        .collect()
}
struct Solo {
    device: Device,
    runtime: SoloRuntime,
    gauge: BmsGauge,
    bgm: BgmFeeder,
    presentation: NativeAudioPresentation,
    pause: NativePause,
    end: Option<NativeEnd>,
    completion: Option<crate::completion::SongCompletion>,
    capture: Option<LiveReplayCapture>,
    competition: Option<Observer>,
    delivery: InputDeliveryTelemetry,
    pre: u64,
    merger: InputMerger,
    host: Host,
    selected: ResolvedPlayPolicy,
}
impl Solo {
    fn new(mode: Mode, finite: bool, classified: bool) -> Self {
        let source = source(false);
        let selected = policy(&source, classified);
        let (device, producer, host_port) = device(16, finite, mode, vec![DeviceId(u64::MAX)]);
        let judge = JudgeEngine::new(
            source.compile().unwrap().chart,
            source.rules(),
            selected.judge().clone(),
        )
        .unwrap();
        let capture = LiveReplayCapture::new_with_policy(
            &judge,
            ClockDomainId(3),
            limits(),
            Timestamp::ZERO,
            0,
            finite.then_some(Timestamp::from_nanos(60_000_000)),
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            &selected,
        )
        .unwrap();
        let competition = Some(Observer {
            header: capture.header().clone(),
            times: vec![],
            marks: 0,
        });
        let mut runtime = SoloRuntime::new(
            ClockDomainId(3),
            ClockDomainId(2),
            Transport::new(logical(0).timestamp, Timestamp::ZERO, Rate::NORMAL),
            bindings(None, false),
            judge,
            producer,
            sounds(&source, 0),
            8,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        if finite {
            runtime
                .set_song_end(Timestamp::from_nanos(60_000_000))
                .unwrap();
        }
        let pause = NativePause::new(raw(0), ClockDomainId(1), 1000).unwrap();
        let pause = if finite {
            pause.with_playback_end_frame(60).unwrap()
        } else {
            pause
        };
        Self {
            device,
            runtime,
            gauge: BmsGauge::new(selected.gauge().try_copy().unwrap()),
            bgm: BgmFeeder::new(
                vec![],
                BgmConfig {
                    output_origin: raw(0),
                    sample_rate: 1000,
                    preroll: Duration::ZERO,
                    lookahead: Duration::from_nanos(100_000_000),
                    max_pending: 2,
                },
            )
            .unwrap(),
            presentation: native_presentation(),
            pause,
            end: finite.then(|| NativeEnd::new(raw(0), ClockDomainId(1), 1000, 60).unwrap()),
            completion: None,
            capture: Some(capture),
            competition,
            delivery: InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap(),
            pre: 0,
            merger: InputMerger::new_dynamic(ClockDomainId(1), host(0), 4096, 16).unwrap(),
            host: host_port,
            selected,
        }
    }
    fn run(
        &mut self,
        scored: bool,
    ) -> (
        NativeGameplayResult<Option<CompletedPlayResult>>,
        ScoreSummary,
    ) {
        let session = AudioGameplaySession {
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
        };
        let mut score = ScoreSummary::default();
        let cfg = config(
            self.device.finite,
            matches!(self.device.mode, Mode::PausePoint | Mode::PauseAsio),
        );
        let result = if scored {
            run_gameplay_audio_with_policy_and_result_and_score_and_ports(
                &mut self.device,
                session,
                cfg,
                &mut Control::default(),
                &mut self.host,
                &mut score,
                &self.selected,
            )
        } else {
            run_gameplay_audio_with_result_and_ports(
                &mut self.device,
                session,
                cfg,
                &mut Control::default(),
                &mut self.host,
            )
        };
        (result, score)
    }
}

#[test]
fn actual_audio_pump_joins_backlog_maps_original_input_and_selected_score_without_host_rate_correction()
 {
    let mut f = Solo::new(Mode::Normal, false, true);
    let initial = f.runtime.transport_mut().anchor();
    let (result, score) = f.run(true);
    assert!(result.unwrap().is_none());
    assert_eq!(score.hits, 1);
    assert_eq!(score.misses, 0);
    assert_eq!(f.host.prepared, vec![PlayerId(1)]);
    assert_eq!(f.merger.source_count(), 1);
    assert_eq!(f.merger.pending(), 0);
    let input_reports = f
        .host
        .reports
        .iter()
        .filter(|r| r.input.is_some())
        .collect::<Vec<_>>();
    assert_eq!(input_reports.len(), 2);
    for report in input_reports {
        assert_eq!(report.song_time, Timestamp::from_nanos(20_000_000));
        assert_eq!(report.input_mapping_quality, ClockMappingQuality::Unknown);
        let meta = report.input.as_ref().unwrap().meta();
        assert_eq!(meta.original_clock_point, Some(host(10_000_000)));
        assert_eq!(meta.source, DeviceId(u64::MAX));
        assert_eq!(meta.clock_domain, ClockDomainId(3));
        assert_eq!(report.audio_at.domain, ClockDomainId(2));
        assert_eq!(report.audio_at, raw(40_000_000));
    }
    assert_eq!(f.runtime.transport_mut().anchor(), initial);
    assert_eq!(initial.host_time, logical(0).timestamp);
    assert_eq!(initial.rate, Rate::NORMAL);
    assert!(f.device.pcm.iter().any(|value| *value == 0.25));
    assert_eq!(
        f.selected
            .judgments()
            .unwrap()
            .project(&score)
            .unwrap()
            .ex_score,
        2
    );
    let file = f.capture.take().unwrap().into_file();
    assert_eq!(file.header.normalized_clock, ClockDomainId(3));
    assert_eq!(
        file.records
            .iter()
            .filter(|r| matches!(r.operation, ReplayOperation::Input(_)))
            .count(),
        2
    );
    assert_eq!(
        f.presentation.authority().committed_input_host(),
        Some(host(10_000_000))
    );
}

#[test]
fn startup_missing_single_anchor_and_stationary_output_do_not_follow_generated_cursor_or_host() {
    for mode in [Mode::Missing, Mode::OneAnchor] {
        let mut f = Solo::new(mode, false, false);
        assert!(f.run(false).0.unwrap().is_none());
        assert_eq!(f.runtime.judge().effective_song_time(), None);
        assert!(f.merger.pending() > 0);
        assert_eq!(f.presentation.authority().committed_operation(), None);
        assert!(f.device.mixer.frame_cursor() > 0);
    }
    let mut f = Solo::new(Mode::Stationary, false, false);
    assert!(f.run(false).0.unwrap().is_none());
    assert_eq!(
        f.runtime.judge().effective_song_time(),
        Some(Timestamp::from_nanos(20_000_000))
    );
    assert_eq!(
        f.presentation.authority().committed_operation(),
        Some(logical(20_000_000))
    );
    assert!(f.device.mixer.frame_cursor() > 20);
    assert!(
        f.host
            .reports
            .iter()
            .all(|r| r.song_time <= Timestamp::from_nanos(20_000_000))
    );
}

#[test]
fn stale_native_relation_fails_before_held_original_input_is_fabricated_or_retimed() {
    let mut f = Solo::new(Mode::Stale, false, false);
    assert!(f.run(false).0.is_err());
    assert_eq!(f.runtime.judge().effective_song_time(), None);
    assert!(f.merger.pending() > 0);
    assert_eq!(f.presentation.authority().committed_input_host(), None);
    assert_eq!(f.presentation.authority().committed_operation(), None);
}

#[test]
fn committed_hit_authority_and_capture_survive_later_host_observer_refusal_once() {
    let mut f = Solo::new(Mode::Normal, false, true);
    f.host.refuse_hit = true;
    let (result, score) = f.run(true);
    assert!(result.is_err());
    assert_eq!(score.hits, 1);
    assert_eq!(
        f.presentation.authority().committed_operation(),
        Some(logical(20_000_000))
    );
    assert_eq!(
        f.presentation.authority().committed_input_host(),
        Some(host(10_000_000))
    );
    let file = f.capture.take().unwrap().into_file();
    assert_eq!(
        file.records
            .iter()
            .filter(|r| matches!(r.operation, ReplayOperation::Input(_)))
            .count(),
        1
    );
    assert_eq!(
        f.host
            .reports
            .iter()
            .flat_map(|r| &r.judge_events)
            .filter(|e| matches!(e.outcome, JudgeOutcome::Hit { .. }))
            .count(),
        1
    );
}

#[test]
fn genuine_point_and_asio_pause_resume_keep_correlation_and_use_raw_logical_control_boundaries() {
    for mode in [Mode::PausePoint, Mode::PauseAsio] {
        let mut f = Solo::new(mode, false, false);
        assert!(f.run(false).0.unwrap().is_none());
        assert!(f.host.pause.contains(&PauseState::Paused));
        assert!(f.host.pause.contains(&PauseState::Running));
        assert_eq!(f.pause.epoch(), 0);
        let anchors = f.runtime.transport_mut().anchors().to_vec();
        assert!(anchors.iter().all(|a| a.host_time < host(0).timestamp));
        assert!(
            anchors
                .iter()
                .any(|a| a.host_time == logical(50_000_000).timestamp && a.rate == Rate::ZERO)
        );
        assert!(
            anchors
                .iter()
                .any(|a| a.host_time == logical(100_000_000).timestamp && a.rate == Rate::NORMAL)
        );
        let post = f
            .host
            .reports
            .iter()
            .find(|r| {
                r.input
                    .as_ref()
                    .is_some_and(|i| i.meta().original_clock_point == Some(host(65_000_000)))
            })
            .unwrap();
        assert_eq!(post.song_time, Timestamp::from_nanos(80_000_000));
        assert!(
            post.judge_events
                .iter()
                .any(|e| matches!(e.outcome, JudgeOutcome::Hit { .. }))
        );
        assert_eq!(f.presentation.authority().epoch().id, 0);
        assert!(
            f.presentation
                .authority()
                .committed_input_host()
                .unwrap()
                .timestamp
                >= host(65_000_000).timestamp
        );
    }
}

#[test]
fn finite_actual_output_end_input_prefix_and_stops_complete_once_on_logical_timeline() {
    let mut f = Solo::new(Mode::Normal, true, true);
    let (result, score) = f.run(true);
    assert!(result.unwrap().is_some());
    assert_eq!(score.hits, 1);
    assert_eq!(score.misses, 0);
    assert_eq!(f.host.completed.len(), 1);
    assert_eq!(
        f.runtime.judge().effective_song_time(),
        Some(Timestamp::from_nanos(60_000_000))
    );
    let closed = f.presentation.authority().closed_host_prefix().unwrap();
    assert_eq!(closed.domain, host(30_000_000).domain);
    assert!(closed.timestamp >= host(30_000_000).timestamp);
    assert_eq!(
        f.device.report.unwrap().playback_end_physical_frame,
        Some(60)
    );
    assert!(
        f.device.report.unwrap().counters.commands_consumed >= f.runtime.admitted_audio_commands()
    );
    assert_eq!(f.competition.as_ref().unwrap().marks, 1);
}

#[test]
fn logical_capture_policy_mismatch_is_refused_before_device_and_score_effects() {
    let mut f = Solo::new(Mode::Normal, false, true);
    f.capture = Some(
        LiveReplayCapture::new_with_policy(
            f.runtime.judge(),
            ClockDomainId(1),
            limits(),
            Timestamp::ZERO,
            0,
            None,
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            &f.selected,
        )
        .unwrap(),
    );
    let (result, score) = f.run(true);
    assert!(result.is_err());
    assert_eq!(f.device.step, 0);
    assert_eq!(score, ScoreSummary::default());
    assert_eq!(f.presentation.authority().history_len(), 0);
    assert_eq!(f.runtime.judge().effective_song_time(), None);
}

#[test]
fn actual_local_reported_audio_failure_commits_processed_member_prefix_once() {
    let source = source(true);
    let selected = policy(&source, false);
    let (mut device, producer, mut host_port) = device(2, false, Mode::Normal, vec![DeviceId(1)]);
    device.backlog_three = true;
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let sources = [DeviceId(1), DeviceId(u64::MAX)];
    let mut states = Vec::new();
    let mut members = Vec::new();
    for i in 0..2 {
        let judge = JudgeEngine::new(
            source.compile().unwrap().chart,
            source.rules(),
            selected.judge().clone(),
        )
        .unwrap();
        let capture = LiveReplayCapture::new_with_policy(
            &judge,
            ClockDomainId(3),
            limits(),
            Timestamp::ZERO,
            0,
            None,
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            &selected,
        )
        .unwrap();
        states.push(GameplayPlayerState {
            player: ids[i],
            capture: Some(capture),
            competition: None::<Observer>,
            completion: None,
            score: ScoreSummary::default(),
            gauge: BmsGauge::default(),
            last_song: Timestamp::ZERO,
        });
        members.push(MemberConfig {
            player: ids[i],
            device: Some(sources[i]),
            bindings: bindings(Some(sources[i]), true),
            judge,
            sounds: sounds(&source, (i as u64 + 1) * 100),
        });
    }
    let mut group = RuntimeGroup::new(
        ClockDomainId(3),
        ClockDomainId(2),
        Transport::new(logical(0).timestamp, Timestamp::ZERO, Rate::NORMAL),
        producer,
        members,
        8,
        &[],
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    let mut p = native_presentation();
    let mut merger = InputMerger::new(ClockDomainId(1), host(0), sources.to_vec(), 16).unwrap();
    let mut bgm = BgmFeeder::new(
        vec![],
        BgmConfig {
            output_origin: raw(0),
            sample_rate: 1000,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(100_000_000),
            max_pending: 1,
        },
    )
    .unwrap();
    let mut pause = NativePause::new(raw(0), ClockDomainId(1), 1000).unwrap();
    let mut end = None;
    let mut delivery = InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap();
    let mut pre = 0;
    let session = AudioCohortSession {
        group: &mut group,
        network: None::<&mut NoopGroupCompetition>,
        states: &mut states,
        merger: &mut merger,
        bgm: &mut bgm,
        discipline: &mut p,
        pause: &mut pause,
        end: &mut end,
        delivery: &mut delivery,
        pre_origin_inputs: &mut pre,
    };
    assert!(
        run_cohort_audio_with_results_and_ports(
            &mut device,
            session,
            config(false, false),
            &mut Control::default(),
            &mut host_port
        )
        .is_err()
    );
    assert_eq!(states[0].score.hits, 3);
    assert_eq!(states[1].score.hits, 0);
    assert_eq!(
        p.authority().committed_operation(),
        Some(logical(20_000_000))
    );
    assert_eq!(p.authority().committed_input_host(), Some(host(10_000_000)));
    assert!(group.poisoned());
    assert_eq!(
        states[0]
            .capture
            .as_ref()
            .unwrap()
            .records()
            .iter()
            .filter(|r| matches!(r.operation, ReplayOperation::Input(_)))
            .count(),
        3
    );
    assert!(states[1].capture.as_ref().unwrap().records().is_empty());
}

#[test]
fn host_normalized_solo_is_refused_from_actual_runtime_identity_with_capture_disabled() {
    let mut f = Solo::new(Mode::Normal, false, false);
    f.capture = None;
    f.competition = None;
    let (producer, _consumer) = command_queue(16).unwrap();
    let source = source(false);
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        f.selected.judge().clone(),
    )
    .unwrap();
    f.runtime = SoloRuntime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(host(0).timestamp, Timestamp::ZERO, Rate::NORMAL),
        bindings(None, false),
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    assert_eq!(
        f.runtime.clock_domains(),
        Some((ClockDomainId(1), ClockDomainId(2)))
    );
    let before = format!("{:?}", f.presentation.authority());
    assert!(f.run(false).0.is_err());
    assert_eq!(f.device.step, 0);
    assert_eq!(f.runtime.judge().effective_song_time(), None);
    assert_eq!(format!("{:?}", f.presentation.authority()), before);
    assert!(f.host.reports.is_empty());
}

struct Cohort {
    device: Device,
    group: RuntimeGroup,
    states: Vec<GameplayPlayerState<Observer>>,
    merger: InputMerger,
    bgm: BgmFeeder,
    p: NativeAudioPresentation,
    pause: NativePause,
    end: Option<NativeEnd>,
    delivery: InputDeliveryTelemetry,
    pre: u64,
    host: Host,
}
impl Cohort {
    fn new(domain: ClockDomainId) -> Self {
        let source = source(false);
        let selected = policy(&source, false);
        let (device, producer, host_port) = device(
            16,
            false,
            Mode::Normal,
            vec![DeviceId(1), DeviceId(u64::MAX)],
        );
        let mut states = Vec::new();
        let mut members = Vec::new();
        for (i, (id, device)) in [
            (PlayerId(7), DeviceId(1)),
            (PlayerId(u32::MAX), DeviceId(u64::MAX)),
        ]
        .into_iter()
        .enumerate()
        {
            let judge = JudgeEngine::new(
                source.compile().unwrap().chart,
                source.rules(),
                selected.judge().clone(),
            )
            .unwrap();
            let capture = LiveReplayCapture::new_with_policy(
                &judge,
                ClockDomainId(3),
                limits(),
                Timestamp::ZERO,
                0,
                None,
                beatkernel_bms::BmsInputMode::ButtonOnly,
                None,
                &selected,
            )
            .unwrap();
            states.push(GameplayPlayerState {
                player: id,
                capture: Some(capture),
                competition: None,
                completion: None,
                score: ScoreSummary::default(),
                gauge: BmsGauge::default(),
                last_song: Timestamp::ZERO,
            });
            members.push(MemberConfig {
                player: id,
                device: Some(device),
                bindings: bindings(Some(device), false),
                judge,
                sounds: sounds(&source, (i as u64 + 1) * 100),
            });
        }
        let origin = if domain == ClockDomainId(3) {
            logical(0)
        } else {
            host(0)
        };
        let mut group = RuntimeGroup::new(
            domain,
            ClockDomainId(2),
            Transport::new(origin.timestamp, Timestamp::ZERO, Rate::NORMAL),
            producer,
            members,
            8,
            &[],
        )
        .unwrap();
        group.set_processing_clock(RuntimeProcessingClock::Disabled);
        Self {
            device,
            group,
            states,
            merger: InputMerger::new(
                ClockDomainId(1),
                host(0),
                vec![DeviceId(1), DeviceId(u64::MAX)],
                16,
            )
            .unwrap(),
            bgm: BgmFeeder::new(
                vec![],
                BgmConfig {
                    output_origin: raw(0),
                    sample_rate: 1000,
                    preroll: Duration::ZERO,
                    lookahead: Duration::from_nanos(100_000_000),
                    max_pending: 2,
                },
            )
            .unwrap(),
            p: native_presentation(),
            pause: NativePause::new(raw(0), ClockDomainId(1), 1000).unwrap(),
            end: None,
            delivery: InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap(),
            pre: 0,
            host: host_port,
        }
    }
    fn run(&mut self) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
        let session = AudioCohortSession {
            group: &mut self.group,
            network: None::<&mut NoopGroupCompetition>,
            states: &mut self.states,
            merger: &mut self.merger,
            bgm: &mut self.bgm,
            discipline: &mut self.p,
            pause: &mut self.pause,
            end: &mut self.end,
            delivery: &mut self.delivery,
            pre_origin_inputs: &mut self.pre,
        };
        run_cohort_audio_with_results_and_ports(
            &mut self.device,
            session,
            config(false, false),
            &mut Control::default(),
            &mut self.host,
        )
    }
}
#[test]
fn actual_cohort_audio_pump_retains_original_ids_independent_scores_and_logical_captures() {
    let mut f = Cohort::new(ClockDomainId(3));
    let anchor = f.group.transport().anchor();
    assert!(f.run().unwrap().is_none());
    assert_eq!(
        f.states.iter().map(|s| s.player).collect::<Vec<_>>(),
        vec![PlayerId(7), PlayerId(u32::MAX)]
    );
    for (state, source) in f.states.iter().zip([DeviceId(1), DeviceId(u64::MAX)]) {
        assert_eq!(state.score.hits, 1);
        assert_eq!(state.score.misses, 0);
        let capture = state.capture.as_ref().unwrap();
        assert_eq!(capture.header().normalized_clock, ClockDomainId(3));
        let inputs = capture
            .records()
            .iter()
            .filter_map(|r| match &r.operation {
                ReplayOperation::Input(i) => Some(i),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(inputs.len(), 2);
        for i in inputs {
            assert_eq!(i.physical.meta().source, source);
            assert_eq!(
                i.physical.meta().original_clock_point,
                Some(host(10_000_000))
            );
        }
    }
    assert_eq!(f.group.transport().anchor(), anchor);
    assert_eq!(f.merger.pending(), 0);
    assert!(f.device.pcm.iter().any(|v| *v == 0.5));
}
#[test]
fn host_normalized_cohort_is_refused_from_real_group_identity_before_device_effects() {
    let mut f = Cohort::new(ClockDomainId(1));
    for state in &mut f.states {
        state.capture = None;
    }
    assert_eq!(
        f.group.clock_domains(),
        Some((ClockDomainId(1), ClockDomainId(2)))
    );
    let before = format!("{:?}", f.p.authority());
    assert!(f.run().is_err());
    assert_eq!(f.device.step, 0);
    assert_eq!(format!("{:?}", f.p.authority()), before);
    for state in &f.states {
        assert_eq!(state.score, ScoreSummary::default());
        assert_eq!(
            f.group
                .member_judge(state.player)
                .unwrap()
                .effective_song_time(),
            None
        );
    }
}

#[test]
fn native_future_asio_association_keeps_older_covered_authority_without_advancing_to_future() {
    let mut f = Solo::new(Mode::FutureAsio, false, false);
    assert!(f.run(false).0.unwrap().is_none());
    assert!(
        f.presentation
            .authority()
            .latest_observation()
            .unwrap()
            .target
            .timestamp
            > f.device.now().timestamp
    );
    assert_eq!(
        f.runtime.judge().effective_song_time(),
        Some(Timestamp::from_nanos(30_000_000))
    );
    assert!(
        f.host
            .reports
            .iter()
            .all(|r| r.song_time <= Timestamp::from_nanos(30_000_000))
    );
    assert_eq!(
        f.host
            .reports
            .iter()
            .flat_map(|r| &r.judge_events)
            .filter(|e| matches!(e.outcome, JudgeOutcome::Hit { .. }))
            .count(),
        1
    );
    assert!(
        f.presentation
            .authority()
            .closed_host_prefix()
            .unwrap()
            .timestamp
            <= f.device.now().timestamp
    );
}
