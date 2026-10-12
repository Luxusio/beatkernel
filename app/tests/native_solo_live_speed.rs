//! Public original-audio solo owner contracts. The device renders a real Mixer;
//! supplied native pairs and collector cuts are independent original evidence.
use beatkernel::{
    audio::{
        AudioFormat, Mixer, OutputFrameBasis, PcmLimits, PcmSample, RenderReport, SampleBank,
        SampleId, VoiceId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeStage},
    replay::codec::ReplayCodecLimits,
    runtime::{RuntimeProcessingClock, RuntimeReport, SoundBinding},
    telemetry::InputDeliveryTelemetry,
    time::{
        presentation::PresentationEstimator, ClockDomainId, ClockPair, ClockPoint, Duration,
        ExtrapolationPolicy, Timestamp,
    },
    transport::Transport,
};
use beatkernel_bms_runtime::{
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    bgm::BgmFeeder,
    competition::ScoreSummary,
    completion::SongCompletion,
    gameplay_competition::NoopSoloCompetition,
    gameplay_presentation::GameplayDevice,
    gauge::BmsGauge,
    live_pause::LivePauseObservation,
    live_speed::LiveSpeedSetup,
    local_input::InputMerger,
    local_runtime::SoloRuntime,
    native_audio::{prepare_audio_with_speed, NativeAudioConfig},
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
    native_end::{EndBoundary, NativeEnd},
    native_gameplay::{
        run_gameplay_audio_with_policy_and_result_and_score_and_ports_with_speed,
        run_gameplay_audio_with_result_and_ports,
        run_gameplay_audio_with_result_and_ports_with_speed, AudioGameplayConfig,
        AudioGameplaySession, GameplaySession, InputBatch, NativeGameplayConfig,
        NativeGameplayResult,
    },
    native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost, PauseState},
    native_pump_control::NativePumpControl,
    play_policy::ResolvedPlayPolicy,
    play_result::CompletedPlayResult,
    playback_pause::NativePause,
    replay_capture::LiveReplayCapture,
    replay_playback::decode_section_setup,
};
use beatkernel_platform::audio::presentation::validation::{
    NativePresentationValidator, OriginalNativePresentationEvidence,
};
use std::{cell::Cell, collections::VecDeque, rc::Rc};

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    point(1, 10_000_000_000 + ns)
}
fn raw(ns: i64) -> ClockPoint {
    point(2, 1_000_000_000 + ns)
}
fn logical(ns: i64) -> ClockPoint {
    point(3, 5_000_000_000 + ns)
}
fn setup(fraction: &str, end: Option<i64>) -> LiveSpeedSetup {
    LiveSpeedSetup::new(
        Timestamp::ZERO,
        Duration::from_nanos(1),
        fraction.parse().unwrap(),
        end.map(ts),
    )
    .unwrap()
}

#[derive(Clone, Copy, Debug)]
enum TerminalMutation {
    Duplicate,
    ReclassifyExpiry,
    CountersDecrease,
    SettledOverflow,
    SettledExceedsConsumed,
    ConsumedWithoutExpiry,
    ExpiryWithoutConsumed,
    PendingIncrease,
    SongDrift,
    VoiceDrift,
    AppliedIncrease,
    LateIncrease,
    PausedLost,
    MarkerLost,
}
const TERMINAL_MUTATIONS: [TerminalMutation; 13] = [
    TerminalMutation::ReclassifyExpiry,
    TerminalMutation::CountersDecrease,
    TerminalMutation::SettledOverflow,
    TerminalMutation::SettledExceedsConsumed,
    TerminalMutation::ConsumedWithoutExpiry,
    TerminalMutation::ExpiryWithoutConsumed,
    TerminalMutation::PendingIncrease,
    TerminalMutation::SongDrift,
    TerminalMutation::VoiceDrift,
    TerminalMutation::AppliedIncrease,
    TerminalMutation::LateIncrease,
    TerminalMutation::PausedLost,
    TerminalMutation::MarkerLost,
];

/// This script alters only a later owner-facing read. Its first marker is an
/// untouched report from this fixture's actual Mixer, including real expiry.
/// The returned-read trace distinguishes owner publication from race_reports.
struct TerminalScript {
    mutation: TerminalMutation,
    gaps: bool,
    marker: Option<RenderReport>,
    stage: usize,
    reads: Vec<Option<RenderReport>>,
    owner_marker_seen: bool,
}
impl TerminalScript {
    fn new(mutation: TerminalMutation, gaps: bool) -> Self {
        Self {
            mutation,
            gaps,
            marker: None,
            stage: 0,
            reads: vec![],
            owner_marker_seen: false,
        }
    }
    fn incomplete(&self) -> bool {
        self.marker.is_some() && self.stage < if self.gaps { 4 } else { 2 }
    }
    fn read(&mut self, actual: Option<RenderReport>) -> Option<RenderReport> {
        if self.marker.is_none() {
            if let Some(report) = actual.filter(|r| {
                r.playback_end_physical_frame.is_some() && r.counters.commands_expired == 1
            }) {
                assert_eq!(report.counters.commands_consumed, 2);
                assert_eq!(report.counters.commands_applied, 1);
                assert_eq!(report.pending_commands, 0);
                self.marker = Some(report);
                self.stage = 1;
                self.reads.push(Some(report));
                return Some(report);
            }
            return actual;
        }
        let marker = self.marker.unwrap();
        let value = if self.gaps && self.stage == 1 {
            None
        } else if self.gaps && self.stage == 2 {
            Some(marker)
        } else if self.stage == if self.gaps { 3 } else { 1 } {
            let mut forged = marker;
            match self.mutation {
                TerminalMutation::Duplicate => {}
                TerminalMutation::ReclassifyExpiry => {
                    forged.counters.commands_applied = 2;
                    forged.counters.commands_expired = 0;
                }
                TerminalMutation::CountersDecrease => {
                    forged.counters.commands_consumed = 1;
                    forged.counters.commands_expired = 0;
                }
                TerminalMutation::SettledOverflow => {
                    forged.counters.commands_applied = u64::MAX;
                    forged.counters.commands_consumed = u64::MAX;
                }
                TerminalMutation::SettledExceedsConsumed => forged.counters.commands_expired = 2,
                TerminalMutation::ConsumedWithoutExpiry => forged.counters.commands_consumed = 3,
                TerminalMutation::ExpiryWithoutConsumed => {
                    forged.counters.commands_applied = 0;
                    forged.counters.commands_expired = 2;
                }
                TerminalMutation::PendingIncrease => forged.pending_commands = 1,
                TerminalMutation::SongDrift => forged.song_position = ts(1),
                TerminalMutation::VoiceDrift => forged.active_voices += 1,
                TerminalMutation::AppliedIncrease => {
                    forged.counters.commands_applied = 2;
                    forged.counters.commands_consumed = 3;
                }
                TerminalMutation::LateIncrease => forged.counters.late_commands += 1,
                TerminalMutation::PausedLost => forged.paused = false,
                TerminalMutation::MarkerLost => forged.playback_end_physical_frame = None,
            }
            Some(forged)
        } else {
            actual
        };
        self.stage += 1;
        self.reads.push(value);
        value
    }
    fn assert_delivered(&self) {
        assert!(
            self.owner_marker_seen,
            "native owner forwarded original marker to its end authority before forgery"
        );
        let marker = self
            .marker
            .expect("original settled Mixer marker delivered to owner");
        assert_eq!(self.reads[0], Some(marker));
        let forged_index = if self.gaps { 3 } else { 1 };
        if self.gaps {
            assert_eq!(self.reads[1], None);
            assert_eq!(self.reads[2], Some(marker));
        }
        assert!(
            self.reads.len() > forged_index,
            "later native query must receive the test report"
        );
    }
}

struct Device {
    mixer: Mixer,
    basis: OutputFrameBasis,
    report: Option<RenderReport>,
    blocks: usize,
    cancelled: Rc<Cell<bool>>,
    desired_pause: Rc<Cell<bool>>,
    exercise_pause: bool,
    no_presentation: bool,
    no_prefix: bool,
    no_report: bool,
    interval: bool,
    creation_pair: Option<ClockPair>,
    pcm: Vec<f32>,
    terminal_race: bool,
    race_armed: bool,
    race_reports: Vec<RenderReport>,
    race_input_published: Rc<Cell<bool>>,
    shifted_origin: bool,
    terminal_script: Option<TerminalScript>,
}
impl GameplayDevice for Device {
    type Presentation = PresentationEstimator;
    fn observe(&mut self, _: &mut Self::Presentation) -> NativeGameplayResult<()> {
        panic!("legacy correction must not run")
    }
    fn observe_audio(&mut self, owner: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        if self.blocks == 0 && !self.no_presentation && !self.interval {
            let pair = ClockPair {
                source: raw(0),
                target: host(if self.shifted_origin { 10_000_000 } else { 0 }),
            };
            owner.admit(NativeAudioSnapshot {
                epoch: 0,
                basis: self.basis,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(pair),
            })?;
            self.creation_pair = Some(pair);
        }
        self.blocks += 1;
        if self.blocks > 40 {
            return Err("bounded fixture exceeded 40 blocks".into());
        }
        let mut pcm = [0.0; 5];
        self.report = Some(self.mixer.render(&mut pcm)?);
        self.pcm.extend(pcm);
        if self.blocks == 40 {
            self.cancelled.set(true);
        }
        if !self.no_presentation {
            let evidence = if self.interval {
                use beatkernel_platform::audio::asio::{
                    AsioPresentationObservation, MultimediaHostInterval,
                };
                let report = self.report.unwrap();
                let elapsed = report.start_frame as i64 * 1_000_000;
                OriginalNativePresentationEvidence::Asio {
                    observation: AsioPresentationObservation::from_render(
                        report,
                        1000,
                        MultimediaHostInterval {
                            before: host(elapsed - 1000),
                            after: host(elapsed + 1000),
                        },
                        0,
                        0,
                        raw(0),
                    )?,
                    basis: Some(self.basis),
                }
            } else {
                OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                    source: raw(self.blocks as i64 * 5_000_000),
                    target: host(
                        self.blocks as i64 * 5_000_000
                            + if self.shifted_origin { 10_000_000 } else { 0 },
                    ),
                })
            };
            owner.admit(NativeAudioSnapshot {
                epoch: 0,
                basis: self.basis,
                evidence,
            })?;
        }
        Ok(())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        if self.race_armed {
            self.race_armed = false;
            let prior = self.report.expect("actual prior callback");
            self.race_reports.push(prior);
            let mut pcm = [0.0; 5];
            let crossing = self.mixer.render(&mut pcm)?;
            self.pcm.extend(pcm);
            self.race_reports.push(crossing);
            self.report = Some(crossing);
            return Ok(Some(prior));
        }
        if self.terminal_race && !self.race_input_published.get() {
            if let Some(prior) = self.race_reports.first() {
                return Ok(Some(*prior));
            }
        }
        let actual = if self.no_report { None } else { self.report };
        Ok(match self.terminal_script.as_mut() {
            Some(script) => script.read(actual),
            None => actual,
        })
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(host(
            self.blocks as i64 * 5_000_000 + if self.shifted_origin { 10_000_000 } else { 0 },
        ))
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        if self.shifted_origin && self.blocks == 2 {
            for (sequence, state) in [(90, ButtonState::Down), (91, ButtonState::Up)] {
                events.push_back(PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(8), host(9_000_000), sequence),
                    control: PhysicalControlId::keyboard(7),
                    state,
                }));
            }
        }
        if self.exercise_pause && self.blocks == 5 {
            self.desired_pause.set(true);
        }
        if self.exercise_pause && self.blocks == 11 {
            self.desired_pause.set(false);
        }
        if self.blocks == 3 || self.blocks == 4 {
            self.race_armed = self.terminal_race && self.blocks == 3;
            events.push_back(PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(DeviceId(9), host(10_000_000), (self.blocks - 3) as u64),
                control: PhysicalControlId::keyboard(7),
                state: if self.blocks == 3 {
                    ButtonState::Down
                } else {
                    ButtonState::Up
                },
            }));
        }
        Ok(InputBatch {
            completed_through: (!self.no_prefix
                && !self
                    .terminal_script
                    .as_ref()
                    .is_some_and(TerminalScript::incomplete))
            .then_some(self.host_now()?),
            backlog: self.blocks == 3,
            closed: false,
        })
    }
    fn audio_pause_observation(
        &mut self,
        owner: &NativeAudioPresentation,
        _: ClockPoint,
    ) -> NativeGameplayResult<LivePauseObservation> {
        Ok(LivePauseObservation::Point(
            owner
                .latest_record()
                .ok_or("missing original pause observation")?
                .pair(),
        ))
    }
    fn publish_paused_audio_output(
        &mut self,
        _: beatkernel_bms_runtime::gameplay_presentation::GameplayAudioOutputContext<'_>,
        _: ClockPoint,
    ) -> NativeGameplayResult<bool> {
        Ok(false)
    }
    fn observe_audio_end(
        &mut self,
        end: &mut NativeEnd,
        owner: &NativeAudioPresentation,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        if let Some(script) = self.terminal_script.as_mut() {
            if script.marker.is_some() && report == script.marker {
                script.owner_marker_seen = true;
            }
        }
        if let Some(pair) = self.creation_pair.take() {
            end.prime(None, pair)?;
        }
        match owner.latest_record() {
            Some(record) => match *record.evidence() {
                OriginalNativePresentationEvidence::Asio { observation, .. } => {
                    Ok(end.observe_asio(observation)?)
                }
                _ => Ok(end.observe(report, record.pair())?),
            },
            None => Ok(None),
        }
    }
    fn observe_end(
        &mut self,
        _: &mut NativeEnd,
        _: &Self::Presentation,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        panic!("legacy end must not run")
    }
    fn seed_resume(
        &mut self,
        _: &mut Self::Presentation,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        panic!("legacy resume must not run")
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        panic!("physical scheduling must remain explicit")
    }
}

#[derive(Default)]
struct Control {
    calls: usize,
}
impl NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        self.calls += 1;
        Err("unbounded playback has no diagnostic timer".into())
    }
    fn checked_add(value: u64, duration: std::time::Duration) -> Option<u64> {
        value.checked_add(u64::try_from(duration.as_nanos()).ok()?)
    }
    fn wait(&mut self, _: std::time::Duration) -> NativeGameplayResult<()> {
        self.calls += 1;
        Ok(())
    }
}
struct Host {
    cancelled: Rc<Cell<bool>>,
    desired_pause: Rc<Cell<bool>>,
    reports: Vec<RuntimeReport>,
    published: usize,
    pauses: Vec<PauseState>,
    race_input_published: Rc<Cell<bool>>,
}
impl NativeGameplayHost for Host {
    fn cancelled(&self) -> bool {
        self.cancelled.get()
    }
    fn pause_requested(&self) -> bool {
        self.desired_pause.get()
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, state: PauseState) {
        self.pauses.push(state);
    }
    fn publish_section_end(&mut self, _: Timestamp) {
        self.published += 1;
    }
    fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        if report.input.is_some() {
            self.race_input_published.set(true);
        }
        self.reports.push(report.clone());
        Ok(())
    }
    fn publish_local_reports(
        &mut self,
        _: &[beatkernel_bms_runtime::local_runtime::PlayerReport],
    ) -> NativeGameplayResult<()> {
        panic!("solo only")
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
}

struct Fixture {
    selected: LiveSpeedSetup,
    config: AudioGameplayConfig,
    device: Device,
    runtime: SoloRuntime,
    gauge: BmsGauge,
    bgm: BgmFeeder,
    presentation: NativeAudioPresentation,
    pause: NativePause,
    end: Option<NativeEnd>,
    completion: Option<SongCompletion>,
    capture: Option<LiveReplayCapture>,
    competition: Option<NoopSoloCompetition>,
    delivery: InputDeliveryTelemetry,
    pre: u64,
    merger: InputMerger,
    host: Host,
    control: Control,
    policy: ResolvedPlayPolicy,
}
impl Fixture {
    fn new(fraction: &str, finite: Option<i64>) -> Self {
        Self::with_preroll(fraction, finite, Duration::from_nanos(1))
    }
    fn with_preroll(fraction: &str, finite: Option<i64>, preroll: Duration) -> Self {
        let selected = LiveSpeedSetup::new(
            Timestamp::ZERO,
            preroll,
            fraction.parse().unwrap(),
            finite.map(ts),
        )
        .unwrap();
        let source = beatkernel_bms::parse(
            "#BPM 120\n#WAV01 note.wav\n#00011:01\n#00112:01\n",
            Default::default(),
        )
        .unwrap();
        let policy = ResolvedPlayPolicy::builtin(30_000_000, 30_000_000, 0).unwrap();
        let judge = JudgeEngine::new(
            source.compile().unwrap().chart,
            source.rules(),
            policy.judge().clone(),
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
                voice: VoiceId(o.id.0 + 100),
                gain: 1.0,
            })
            .collect();
        let limits =
            ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap())
                .unwrap();
        let capture = LiveReplayCapture::new_with_policy_and_speed(
            &judge,
            ClockDomainId(3),
            limits,
            Timestamp::ZERO,
            0,
            finite.map(ts),
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            &policy,
            selected.speed(),
        )
        .unwrap();
        let format = AudioFormat::new(1000, 1).unwrap();
        let pcm_limits = PcmLimits::new(400, 400, 1).unwrap();
        let mut bank = SampleBank::new(format, pcm_limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25; 100], pcm_limits).unwrap(),
        )
        .unwrap();
        let endpoint = selected.playback_end_frame(1000).unwrap();
        let audio = prepare_audio_with_speed(
            bank,
            vec![],
            NativeAudioConfig {
                output_origin: raw(0),
                start: Timestamp::ZERO,
                preroll: selected.preroll(),
                lookahead: Duration::from_nanos(100_000_000),
                voices: 4,
                max_render_frames: 16,
                playback_end_frame: endpoint,
                gated_start: false,
            },
            selected.speed(),
        )
        .unwrap();
        let basis = audio.mixer.output_frame_basis();
        let mut runtime = SoloRuntime::new(
            ClockDomainId(3),
            ClockDomainId(2),
            selected.transport(logical(0).timestamp).unwrap(),
            BindingMap::from_bindings([Binding {
                device: DeviceSelector::Any,
                physical: PhysicalControlId::keyboard(7),
                game_control: GameControlId(0x11),
            }])
            .unwrap(),
            judge,
            audio.producer,
            sounds,
            16,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        if let Some(end) = finite {
            runtime.set_song_end(ts(end)).unwrap();
        }
        let presentation = NativeAudioPresentation::new(
            AudioAuthority::new(
                AudioAuthorityConfig::default(),
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
        .unwrap();
        let pause = NativePause::new(raw(0), ClockDomainId(1), 1000).unwrap();
        let pause = match endpoint {
            Some(end) => pause.with_playback_end_frame(end).unwrap(),
            None => pause,
        };
        let cancelled = Rc::new(Cell::new(false));
        let desired_pause = Rc::new(Cell::new(false));
        let race_input_published = Rc::new(Cell::new(false));
        Self {
            selected,
            config: AudioGameplayConfig {
                gameplay: NativeGameplayConfig {
                    origin: host(0),
                    stream_origin: raw(0),
                    playback_origin: raw(0),
                    song_origin: selected.song_origin(),
                    sample_rate: 1000,
                    end_song: finite.map(ts),
                    advance_lag: Duration::from_nanos(10_000_000),
                    seconds: None,
                    pause_supported: false,
                    logical_schedule: true,
                },
                section_start: Timestamp::ZERO,
            },
            device: Device {
                mixer: audio.mixer,
                basis,
                report: None,
                blocks: 0,
                cancelled: cancelled.clone(),
                desired_pause: desired_pause.clone(),
                exercise_pause: false,
                no_presentation: false,
                no_prefix: false,
                no_report: false,
                interval: false,
                creation_pair: None,
                pcm: vec![],
                terminal_race: false,
                race_armed: false,
                race_reports: vec![],
                race_input_published: race_input_published.clone(),
                shifted_origin: false,
                terminal_script: None,
            },
            runtime,
            gauge: BmsGauge::new(policy.gauge().try_copy().unwrap()),
            bgm: audio.bgm,
            presentation,
            pause,
            end: endpoint.map(|end| NativeEnd::new(raw(0), ClockDomainId(1), 1000, end).unwrap()),
            completion: None,
            capture: Some(capture),
            competition: None,
            delivery: InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap(),
            pre: 0,
            merger: InputMerger::new_dynamic(ClockDomainId(1), host(0), 4096, 16).unwrap(),
            host: Host {
                cancelled,
                desired_pause,
                reports: vec![],
                published: 0,
                pauses: vec![],
                race_input_published,
            },
            control: Control::default(),
            policy,
        }
    }
    fn run(
        &mut self,
        legacy: bool,
        scored: bool,
    ) -> NativeGameplayResult<Option<CompletedPlayResult>> {
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
        if legacy {
            run_gameplay_audio_with_result_and_ports(
                &mut self.device,
                session,
                self.config,
                &mut self.control,
                &mut self.host,
            )
        } else if scored {
            run_gameplay_audio_with_policy_and_result_and_score_and_ports_with_speed(
                &mut self.device,
                session,
                self.config,
                &mut self.control,
                &mut self.host,
                &mut ScoreSummary::default(),
                &self.policy,
                self.selected,
            )
        } else {
            run_gameplay_audio_with_result_and_ports_with_speed(
                &mut self.device,
                session,
                self.config,
                &mut self.control,
                &mut self.host,
                self.selected,
            )
        }
    }
    fn assert_cold_refusal(&mut self, scored: bool) {
        let anchors = self.runtime.transport_mut().anchors().to_vec();
        let gauge = self.gauge.snapshot().clone();
        let bytes = self.capture.as_ref().unwrap().encoded_bytes();
        assert!(self.run(false, scored).is_err());
        assert_eq!(
            self.device.blocks, 0,
            "no output observation before admission"
        );
        assert_eq!(self.control.calls, 0, "no pump effect before admission");
        assert!(self.host.reports.is_empty());
        assert_eq!(self.runtime.transport_mut().anchors(), anchors);
        assert_eq!(self.gauge.snapshot(), &gauge);
        assert_eq!(self.capture.as_ref().unwrap().encoded_bytes(), bytes);
        assert!(self.runtime.judge().effective_song_time().is_none());
        assert_eq!(self.merger.pending(), 0);
    }
}

#[test]
fn four_rates_keep_original_input_and_raw_output_coordinates_in_the_actual_solo_pump() {
    for (fraction, expected) in [
        ("1/2", 4_999_999),
        ("1/1", 9_999_999),
        ("2/1", 19_999_998),
        ("3/2", 14_999_998),
    ] {
        let mut f = Fixture::new(fraction, None);
        assert!(f.run(false, true).unwrap().is_none());
        let inputs: Vec<_> = f
            .host
            .reports
            .iter()
            .filter(|report| report.input.is_some())
            .collect();
        assert_eq!(inputs.len(), 2, "{fraction}");
        for report in inputs {
            assert_eq!(report.song_time, ts(expected), "{fraction}");
            let meta = report.input.as_ref().unwrap().meta();
            assert_eq!(meta.original_clock_point, Some(host(10_000_000)));
            assert_eq!(meta.source, DeviceId(9));
            assert_eq!(meta.clock_domain, ClockDomainId(3));
            assert_eq!(report.audio_at.domain, ClockDomainId(2));
            assert!(report.audio_at.timestamp >= raw(15_000_000).timestamp);
        }
        assert_eq!(f.device.mixer.rate(), f.selected.speed().rate());
        assert!(f.device.pcm.iter().any(|&value| value > 0.0));
        assert_eq!(f.merger.pending(), 0);
        let recorded = decode_section_setup(&f.capture.as_ref().unwrap().header().options).unwrap();
        assert_eq!(recorded.source_speed, f.selected.speed());
        assert!(f.capture.as_ref().unwrap().records().len() >= 2);
    }
}

#[test]
fn actual_native_solo_discards_negative_audio_origin_before_runtime_and_keeps_exact_origin_input() {
    for fraction in ["1/1", "3/2"] {
        let mut f = Fixture::new(fraction, None);
        let mut ac = AudioAuthorityConfig::default();
        ac.input_extrapolation = ExtrapolationPolicy::Bounded {
            before: Duration::from_nanos(20_000_000),
            after: Duration::ZERO,
        };
        f.presentation = NativeAudioPresentation::new(
            AudioAuthority::new(
                ac,
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
        .unwrap();
        f.device.shifted_origin = true;
        f.config.gameplay.advance_lag = Duration::from_nanos(100_000_000);
        assert!(f.run(false, true).unwrap().is_none());
        assert_eq!(f.pre, 2);
        assert_eq!(f.merger.pending(), 0);
        let inputs: Vec<_> = f
            .host
            .reports
            .iter()
            .filter(|r| r.input.is_some())
            .collect();
        assert_eq!(inputs.len(), 2);
        for report in inputs {
            let meta = report.input.as_ref().unwrap().meta();
            assert_eq!(meta.source, DeviceId(9));
            assert_eq!(meta.original_clock_point, Some(host(10_000_000)));
            assert_eq!(report.song_time, f.selected.song_origin());
            assert_eq!(report.audio_at.domain, ClockDomainId(2));
        }
        let capture = f.capture.as_ref().unwrap();
        assert_eq!(
            capture
                .records()
                .iter()
                .filter(|r| matches!(r.operation, beatkernel::replay::ReplayOperation::Input(_)))
                .count(),
            2
        );
    }
}

#[test]
fn actual_solo_stale_schedule_publishes_after_final_callback_and_waits_for_expiry() {
    let mut f = Fixture::new("2/1", Some(30_000_000));
    f.device.terminal_race = true;
    assert!(f.run(false, true).unwrap().is_some());
    let prior = f.device.race_reports[0];
    let crossing = f.device.race_reports[1];
    assert_eq!(prior.playback_end_physical_frame, None);
    assert_eq!(
        prior.playback_start_frame + prior.playback_frames as u64,
        15
    );
    assert_eq!(crossing.playback_end_physical_frame, Some(16));
    assert_eq!(crossing.counters.commands_consumed, 1);
    assert_eq!(crossing.counters.commands_applied, 1);
    assert_eq!(crossing.counters.commands_expired, 0);
    let down = f
        .host
        .reports
        .iter()
        .find(|r| r.input.as_ref().is_some_and(|i| i.meta().sequence == 0))
        .unwrap();
    assert_eq!(down.audio_at, raw(15_000_000));
    assert_eq!(down.song_time, ts(19_999_998));
    assert_eq!(
        down.input.as_ref().unwrap().meta().original_clock_point,
        Some(host(10_000_000))
    );
    assert_eq!(down.judge_events.len(), 1);
    assert!(down.audio_commands.iter().any(|c| matches!(c, beatkernel::audio::AudioCommand::Play { at, .. } if *at == raw(15_000_000).timestamp)));
    let terminal = f.device.report.unwrap();
    assert!(terminal.start_frame >= crossing.counters.rendered_frames);
    assert_eq!(terminal.counters.commands_consumed, 2);
    assert_eq!(terminal.counters.commands_applied, 1);
    assert_eq!(terminal.counters.commands_expired, 1);
    assert_eq!(terminal.counters.settled_commands(), Some(2));
    assert_eq!(terminal.pending_commands, 0);
    assert_eq!(terminal.song_position, crossing.song_position);
    assert_eq!(terminal.active_voices, crossing.active_voices);
    assert!(f.device.pcm.iter().all(|v| *v == 0.0));
    assert_eq!(f.host.published, 1);
    let source = beatkernel_bms::parse(
        "#BPM 120\n#WAV01 note.wav\n#00011:01\n#00112:01\n",
        Default::default(),
    )
    .unwrap();
    let capture = f.capture.as_ref().unwrap();
    let replay = beatkernel_bms_runtime::replay_playback::reconstruct_section(
        &source,
        beatkernel::replay::codec::ReplayFile::new(
            capture.header().clone(),
            capture.records().to_vec(),
        ),
        ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        replay.engine().stable_hash().unwrap(),
        f.runtime.judge().stable_hash().unwrap()
    );
    let mut gauge = BmsGauge::new(f.policy.gauge().try_copy().unwrap());
    gauge.observe(replay.results(), &[]).unwrap();
    assert_eq!(gauge, f.gauge);
}

#[test]
fn native_solo_rejects_impossible_progression_after_owner_observes_real_terminal_marker() {
    for gaps in [false, true] {
        for mutation in TERMINAL_MUTATIONS {
            let mut f = Fixture::new("2/1", Some(30_000_000));
            f.device.terminal_race = true;
            f.device.terminal_script = Some(TerminalScript::new(mutation, gaps));
            let result = f.run(false, true);
            f.device
                .terminal_script
                .as_ref()
                .unwrap()
                .assert_delivered();
            assert!(result.is_err(), "{mutation:?}, intervening None/duplicate={gaps}: native owner accepted impossible terminal evidence");
            // These remain real owner commands and original runtime input facts.
            assert_eq!(f.device.report.unwrap().counters.commands_consumed, 2);
            assert_eq!(f.device.report.unwrap().counters.commands_applied, 1);
            assert_eq!(f.device.report.unwrap().counters.commands_expired, 1);
            let down = f
                .host
                .reports
                .iter()
                .find(|r| r.input.as_ref().is_some_and(|i| i.meta().sequence == 0))
                .unwrap();
            assert_eq!(down.audio_at, raw(15_000_000));
            assert_eq!(down.song_time, ts(19_999_998));
            assert_eq!(
                down.input.as_ref().unwrap().meta().original_clock_point,
                Some(host(10_000_000))
            );
            assert_eq!(down.judge_events.len(), 1);
            assert!(f.device.pcm.iter().all(|v| *v == 0.0));
        }
    }
}

#[test]
fn native_solo_accepts_real_terminal_expiry_duplicates_and_unavailable_intermediate_read() {
    let mut f = Fixture::new("2/1", Some(30_000_000));
    f.device.terminal_race = true;
    f.device.terminal_script = Some(TerminalScript::new(TerminalMutation::Duplicate, true));
    assert!(f.run(false, true).unwrap().is_some());
    f.device
        .terminal_script
        .as_ref()
        .unwrap()
        .assert_delivered();
    assert_eq!(
        f.device.report.unwrap().counters.settled_commands(),
        Some(2)
    );
    assert_eq!(f.host.published, 1);
    assert_eq!(f.merger.pending(), 0);
    assert!(f.device.pcm.iter().all(|v| *v == 0.0));
}

#[test]
fn same_integer_anchor_without_exact_preroll_phase_is_refused_before_effects() {
    for fraction in ["1/2", "1/1", "2/1", "3/2"] {
        for scored in [false, true] {
            let mut f = Fixture::new(fraction, None);
            *f.runtime.transport_mut() = Transport::new(
                logical(0).timestamp,
                f.selected.song_origin(),
                f.selected.speed().rate(),
            );
            f.assert_cold_refusal(scored);
        }
    }
}

#[test]
fn wrong_selected_rate_start_preroll_end_domain_and_source_header_are_atomic() {
    for case in 0..7 {
        for scored in [false, true] {
            let mut f = Fixture::new("3/2", None);
            match case {
                0 => f.selected = setup("2/1", None),
                1 => {
                    f.selected =
                        LiveSpeedSetup::new(ts(1), f.selected.preroll(), f.selected.speed(), None)
                            .unwrap()
                }
                2 => {
                    f.selected = LiveSpeedSetup::new(
                        ts(0),
                        Duration::from_nanos(2),
                        f.selected.speed(),
                        None,
                    )
                    .unwrap()
                }
                3 => f.selected = setup("3/2", Some(99)),
                4 => f.config.gameplay.stream_origin.domain = ClockDomainId(77),
                5 => f.config.section_start = ts(1),
                _ => {
                    let limits = ReplayCodecLimits::new(
                        65536,
                        128,
                        4096,
                        CodecLimits::new(4096, 1024).unwrap(),
                    )
                    .unwrap();
                    f.capture = Some(
                        LiveReplayCapture::new_with_policy_and_speed(
                            f.runtime.judge(),
                            ClockDomainId(3),
                            limits,
                            Timestamp::ZERO,
                            0,
                            None,
                            beatkernel_bms::BmsInputMode::ButtonOnly,
                            None,
                            &f.policy,
                            "1/2".parse().unwrap(),
                        )
                        .unwrap(),
                    );
                }
            }
            f.assert_cold_refusal(scored);
        }
    }
}

#[test]
fn old_audio_entrypoint_refuses_selected_nonnormal_rate_without_touching_device() {
    let mut f = Fixture::new("2/1", None);
    assert!(f.run(true, false).is_err());
    assert_eq!(f.device.blocks, 0);
    assert_eq!(f.control.calls, 0);
}

#[test]
fn cancelled_valid_cold_setup_has_no_output_or_completion_effects() {
    for fraction in ["1/2", "1/1", "2/1", "3/2"] {
        let mut f = Fixture::new(fraction, Some(21_000_000));
        f.host.cancelled.set(true);
        assert!(f.run(false, false).unwrap().is_none());
        assert_eq!(f.device.blocks, 0);
        assert!(f.host.reports.is_empty());
        assert_eq!(f.host.published, 0);
    }
}

#[test]
fn real_finite_mixer_and_native_crossing_close_original_end_at_four_rates() {
    for fraction in ["1/2", "1/1", "2/1", "3/2"] {
        let mut f = Fixture::new(fraction, Some(21_000_000));
        let result = f.run(false, false).unwrap();
        assert!(
            result.is_some(),
            "{fraction}: acquired {:?}, closed {:?}, pending {}, native {:?}, render {:?}",
            f.presentation.authority().acquired_prefix(),
            f.presentation.authority().closed_host_prefix(),
            f.merger.pending(),
            f.presentation.latest_record(),
            f.device.report
        );
        assert_eq!(
            f.runtime.judge().effective_song_time(),
            Some(ts(21_000_000))
        );
        assert_eq!(f.host.published, 1);
        let rendered = f.device.report.unwrap();
        assert!(rendered.playback_end_physical_frame.is_some());
        assert_eq!(rendered.pending_commands, 0);
        assert_eq!(
            rendered.counters.commands_applied,
            rendered.counters.commands_consumed
        );
        if fraction == "2/1" {
            assert_delayed_original_input_after_real_seal(&f);
        }
    }
}

#[test]
fn renderahead_without_original_presentation_never_finalizes_capped_song() {
    let mut f = Fixture::new("2/1", Some(21_000_000));
    f.device.no_presentation = true;
    let result = f.run(false, false);
    assert!(result.is_err() || result.unwrap().is_none());
    assert_eq!(f.host.published, 0);
    assert!(f.runtime.judge().effective_song_time().is_none());
}

#[test]
fn absent_acquisition_prefix_cannot_complete_even_after_real_output_endpoint() {
    let mut f = Fixture::new("2/1", Some(21_000_000));
    f.device.no_prefix = true;
    let result = f.run(false, false);
    assert!(result.is_err() || result.unwrap().is_none());
    assert_eq!(f.host.published, 0);
}

#[test]
fn missing_real_render_ack_cannot_authorize_finite_finalization() {
    let mut f = Fixture::new("3/2", Some(21_000_000));
    f.device.no_report = true;
    let result = f.run(false, false);
    assert!(result.is_err() || result.unwrap().is_none());
    assert_eq!(f.host.published, 0);
}

#[test]
fn original_asio_brackets_keep_four_rates_out_of_legacy_correction_and_complete() {
    for fraction in ["1/2", "1/1", "2/1", "3/2"] {
        let mut f = Fixture::new(fraction, Some(21_000_000));
        f.device.interval = true;
        let result = f.run(false, false).unwrap();
        assert!(
            result.is_some(),
            "{fraction}: acquired {:?}, closed {:?}, pending {}, native {:?}, render {:?}",
            f.presentation.authority().acquired_prefix(),
            f.presentation.authority().closed_host_prefix(),
            f.merger.pending(),
            f.presentation.latest_record(),
            f.device.report
        );
        assert_eq!(
            f.runtime.judge().effective_song_time(),
            Some(ts(21_000_000))
        );
        assert_eq!(f.host.published, 1);
        assert!(matches!(
            *f.presentation.latest_record().unwrap().evidence(),
            OriginalNativePresentationEvidence::Asio { .. }
        ));
        if fraction == "2/1" {
            assert_delayed_original_input_after_real_seal(&f);
        }
    }
}

fn assert_delayed_original_input_after_real_seal(f: &Fixture) {
    use beatkernel::{audio::AudioCommand, judge::JudgeOutcome, replay::ReplayOperation};
    // The original Down at 10ms is transferred after the 5-frame block has
    // physically crossed the 11-frame endpoint. Its judge/capture remains valid;
    // a new sound cannot be admitted to output that actually sealed already.
    let down = f
        .host
        .reports
        .iter()
        .find(|report| {
            report
                .input
                .as_ref()
                .is_some_and(|input| input.meta().sequence == 0)
        })
        .expect("original delayed Down was processed");
    assert_eq!(down.song_time, ts(19_999_998));
    assert_eq!(
        down.input.as_ref().unwrap().meta().original_clock_point,
        Some(host(10_000_000))
    );
    assert!(down
        .judge_events
        .iter()
        .any(|event| matches!(event.outcome, JudgeOutcome::Hit { .. })));
    assert!(
        down.audio_at.timestamp >= raw(11_000_000).timestamp,
        "raw actual output frontier must not be retimed to hide sealing"
    );
    assert!(down
        .audio_commands
        .iter()
        .all(|command| !matches!(command, AudioCommand::Play { .. })));
    assert!(down.audio_failures.is_empty());
    let recorded = f.capture.as_ref().unwrap().records().iter().find(|record| matches!(&record.operation, ReplayOperation::Input(input) if input.physical.meta().sequence == 0)).expect("delayed original Down is retained in the canonical capture");
    assert_eq!(recorded.song_time, ts(19_999_998));
    let ReplayOperation::Input(bound) = &recorded.operation else {
        unreachable!()
    };
    assert_eq!(
        bound.physical.meta().original_clock_point,
        Some(host(10_000_000))
    );
    let rendered = f.device.report.unwrap();
    assert_eq!(rendered.playback_end_physical_frame, Some(11));
    assert_eq!(rendered.pending_commands, 0);
    // The selected SetRate is the only actual command. No new Play/Stop credit
    // may be fabricated or left waiting beyond the immutable output endpoint.
    assert_eq!(rendered.counters.commands_consumed, 1);
    assert_eq!(rendered.counters.commands_applied, 1);
}

#[test]
fn zero_and_quantized_output_endpoints_need_real_native_and_mixer_authority() {
    for (preroll, expected_frames) in [(Duration::ZERO, 0), (Duration::from_nanos(1), 1)] {
        let mut f = Fixture::with_preroll("2/1", Some(1), preroll);
        assert_eq!(
            f.selected.playback_end_frame(1000).unwrap(),
            Some(expected_frames)
        );
        assert!(f.run(false, false).unwrap().is_some());
        assert_eq!(f.runtime.judge().effective_song_time(), Some(ts(1)));
        assert_eq!(f.host.published, 1);
        assert!(f
            .device
            .report
            .unwrap()
            .playback_end_physical_frame
            .is_some());
        let mut missing = Fixture::with_preroll("2/1", Some(1), preroll);
        missing.device.no_presentation = true;
        let result = missing.run(false, false);
        assert!(result.is_err() || result.unwrap().is_none());
        assert_eq!(missing.host.published, 0);
    }
}

#[test]
fn explicit_normal_owner_preserves_legacy_capture_gauge_and_pcm() {
    let mut old = Fixture::with_preroll("1/1", None, Duration::ZERO);
    *old.runtime.transport_mut() = Transport::new(
        logical(0).timestamp,
        Timestamp::ZERO,
        "1/1"
            .parse::<beatkernel_bms_runtime::replay_speed::ReplaySpeed>()
            .unwrap()
            .rate(),
    );
    let mut selected = Fixture::with_preroll("1/1", None, Duration::ZERO);
    assert!(old.run(true, false).unwrap().is_none());
    assert!(selected.run(false, false).unwrap().is_none());
    assert_eq!(old.gauge.snapshot(), selected.gauge.snapshot());
    assert_eq!(old.device.pcm, selected.device.pcm);
    assert_eq!(
        old.capture.take().unwrap().into_bytes().unwrap(),
        selected.capture.take().unwrap().into_bytes().unwrap()
    );
    let a: Vec<_> = old
        .host
        .reports
        .iter()
        .map(|r| (r.song_time, r.audio_at))
        .collect();
    let b: Vec<_> = selected
        .host
        .reports
        .iter()
        .map(|r| (r.song_time, r.audio_at))
        .collect();
    assert_eq!(a, b);
}

#[test]
fn actual_pause_and_resume_preserve_selected_exact_phase_and_original_raw_coordinates() {
    for fraction in ["1/2", "3/2"] {
        let mut f = Fixture::new(fraction, None);
        f.device.exercise_pause = true;
        f.config.gameplay.pause_supported = true;
        let initial_setup = f.runtime.transport_mut().preroll_setup();
        assert!(f.run(false, false).unwrap().is_none());
        assert!(f.host.pauses.contains(&PauseState::Paused));
        assert!(f.host.pauses.contains(&PauseState::Running));
        assert_eq!(f.runtime.transport_mut().preroll_setup(), initial_setup);
        assert!(f.runtime.transport_mut().anchors().len() >= 3);
        let input = f
            .host
            .reports
            .iter()
            .find(|report| report.input.is_some())
            .unwrap();
        assert_eq!(
            input.input.as_ref().unwrap().meta().original_clock_point,
            Some(host(10_000_000))
        );
        assert_eq!(input.audio_at.domain, ClockDomainId(2));
        assert_eq!(f.merger.pending(), 0);
        assert_eq!(f.host.published, 0);
    }
}

/// Original target-rate presentation is independent of source callbacks and
/// generated/admitted PCM. This fixture can hold its native counter below end.
struct ConvertedDevice {
    output: beatkernel_platform::audio::ConvertedNativeOutputState,
    basis: beatkernel::audio::TargetFrameBasis,
    blocks: usize,
    submitted: u64,
    source_only: bool,
    cancelled: Rc<Cell<bool>>,
}
impl ConvertedDevice {
    fn pair(&self) -> ClockPair {
        let played = if self.source_only {
            self.submitted.min(4)
        } else {
            self.submitted
        };
        ClockPair {
            source: self.basis.point_at_stream_frame(played).unwrap(),
            target: host((played * 1_000_000_000 / 2000) as i64),
        }
    }
}
impl GameplayDevice for ConvertedDevice {
    type Presentation = PresentationEstimator;
    fn observe(&mut self, _: &mut Self::Presentation) -> NativeGameplayResult<()> {
        panic!("converted target does not use legacy correction")
    }
    fn observe_audio(&mut self, owner: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        self.blocks += 1;
        if self.blocks > 40 {
            return Err("bounded converted fixture exceeded 40 blocks".into());
        }
        self.output.render_pending(8)?;
        self.output.admit(8)?;
        self.submitted += 8;
        if self.blocks == 40 {
            self.cancelled.set(true);
        }
        owner.admit_target(
            beatkernel_bms_runtime::native_audio_presentation::TargetNativeAudioSnapshot {
                epoch: 0,
                basis: self.basis,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(self.pair()),
            },
        )?;
        Ok(())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(self.output.last_real_source_report())
    }
    fn audio_pause_observation(
        &mut self,
        owner: &NativeAudioPresentation,
        _: ClockPoint,
    ) -> NativeGameplayResult<LivePauseObservation> {
        Ok(LivePauseObservation::Target {
            epoch: 0,
            basis: self.basis,
            facts: self.output.boundaries(),
            source: self.output.last_real_source_report(),
            pair: owner
                .authority()
                .latest_observation()
                .ok_or("missing original native target observation")?,
        })
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(host((self.submitted * 1_000_000_000 / 2000) as i64))
    }
    fn acquire(
        &mut self,
        _: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        Ok(InputBatch {
            completed_through: Some(self.host_now()?),
            backlog: false,
            closed: false,
        })
    }
    fn observe_audio_end(
        &mut self,
        end: &mut NativeEnd,
        _: &NativeAudioPresentation,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        Ok(end.observe_target(
            0,
            self.basis,
            self.output.boundaries(),
            self.output.last_real_source_report(),
            self.pair(),
        )?)
    }
    fn observe_end(
        &mut self,
        _: &mut NativeEnd,
        _: &Self::Presentation,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        panic!("converted end requires target crossing")
    }
    fn seed_resume(
        &mut self,
        _: &mut Self::Presentation,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        panic!("unpaused converted fixture")
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        panic!("source schedule uses actual render report")
    }
}

fn run_converted(
    f: &mut Fixture,
    source_only: bool,
) -> (
    NativeGameplayResult<Option<CompletedPlayResult>>,
    ConvertedDevice,
) {
    use beatkernel::audio::{ChannelMatrix, ResampleQuality};
    use beatkernel_platform::audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding};
    // Keep the existing fixture structurally whole while transferring unique
    // real Mixer ownership. This unused replacement has a disconnected queue.
    let replacement = Fixture::new("1/1", None).device.mixer;
    let mixer = std::mem::replace(&mut f.device.mixer, replacement);
    let output = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(2000, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        8,
    )
    .unwrap_or_else(|_| panic!("valid real converter setup"));
    let basis = output.target_frame_basis();
    f.presentation = beatkernel_bms_runtime::native_audio_startup::new_target_audio_presentation(
        0,
        basis,
        ClockDomainId(1),
        logical(0),
        AudioAuthorityConfig::default(),
    )
    .unwrap();
    f.pause = f.pause.clone().with_target_basis(0, basis).unwrap();
    f.end = f
        .end
        .take()
        .map(|end| end.with_target_basis(0, basis).unwrap());
    let mut device = ConvertedDevice {
        output,
        basis,
        blocks: 0,
        submitted: 0,
        source_only,
        cancelled: f.host.cancelled.clone(),
    };
    let session = AudioGameplaySession {
        session: GameplaySession {
            runtime: &mut f.runtime,
            gauge: &mut f.gauge,
            bgm: &mut f.bgm,
            discipline: &mut f.presentation,
            pause: &mut f.pause,
            end: &mut f.end,
            completion: &mut f.completion,
            capture: &mut f.capture,
            competition: &mut f.competition,
            delivery: &mut f.delivery,
            pre_origin_inputs: &mut f.pre,
        },
        merger: &mut f.merger,
    };
    let result = run_gameplay_audio_with_result_and_ports_with_speed(
        &mut device,
        session,
        f.config,
        &mut f.control,
        &mut f.host,
        f.selected,
    );
    (result, device)
}

#[test]
fn actual_converted_target_crossing_closes_original_end_at_four_rates() {
    for fraction in ["1/2", "1/1", "2/1", "3/2"] {
        let mut f = Fixture::new(fraction, Some(21_000_000));
        let (result, device) = run_converted(&mut f, false);
        assert!(result.unwrap().is_some(), "{fraction}");
        assert_eq!(
            f.runtime.judge().effective_song_time(),
            Some(ts(21_000_000))
        );
        assert_eq!(f.host.published, 1);
        let endpoint = device.output.boundaries().end.unwrap();
        assert!(
            device.pair().source.timestamp
                >= endpoint
                    .target_time
                    .point(device.basis.origin())
                    .unwrap()
                    .timestamp
        );
        assert!(device
            .output
            .last_real_source_report()
            .unwrap()
            .playback_end_physical_frame
            .is_some());
        assert_eq!(device.output.mixer().rate(), f.selected.speed().rate());
    }
}

#[test]
fn source_endpoint_without_converted_target_native_crossing_never_completes() {
    let mut f = Fixture::new("2/1", Some(21_000_000));
    let (result, device) = run_converted(&mut f, true);
    assert!(result.is_err() || result.unwrap().is_none());
    assert_eq!(f.host.published, 0);
    let endpoint = device
        .output
        .boundaries()
        .end
        .expect("source output really reached and mapped its finite endpoint");
    assert!(device
        .output
        .last_real_source_report()
        .unwrap()
        .playback_end_physical_frame
        .is_some());
    assert!(
        device.pair().source.timestamp
            < endpoint
                .target_time
                .point(device.basis.origin())
                .unwrap()
                .timestamp
    );
}
