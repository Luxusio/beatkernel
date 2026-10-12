//! Independent live cohort contracts over the original public native ports.
//! The device renders a real Mixer; supplied pairs describe that software
//! output, and do not claim physical sound or a sixty-four-device experiment.
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits,
        PcmSample, RenderReport, SampleBank, SampleId, VoiceId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeStage},
    replay::{codec::ReplayCodecLimits, ReplayHeader, ReplayOperation},
    runtime::{RuntimeProcessingClock, RuntimeReport, SoundBinding},
    telemetry::InputDeliveryTelemetry,
    time::{
        presentation::PresentationEstimator, ClockDomainId, ClockPair, ClockPoint, Duration,
        ExtrapolationPolicy, Timestamp,
    },
    transport::{Rate, Transport},
};
use beatkernel_bms_runtime::{
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    bgm::{BgmConfig, BgmFeeder},
    competition::ScoreSummary,
    gameplay_competition::{GroupCompetitionPort, SoloCompetitionPort},
    gameplay_presentation::GameplayDevice,
    gauge::{BmsGauge, GaugeProfile},
    live_speed::LiveSpeedSetup,
    local_input::InputMerger,
    local_players::PlayerId,
    local_runtime::{MemberConfig, PlayerReport, RuntimeGroup},
    multiplayer_group::MemberProgress,
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
    native_cohort::{
        run_cohort_audio_with_ports_and_speed, AudioCohortSession, GameplayPlayerState,
    },
    native_end::{EndBoundary, NativeEnd},
    native_gameplay::{
        AudioGameplayConfig, InputBatch, NativeGameplayConfig, NativeGameplayResult,
    },
    native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost, PauseState},
    native_pump_control::NativePumpControl,
    play_result::CompletedPlayResult,
    playback_pause::NativePause,
    replay_capture::LiveReplayCapture,
    replay_playback::{decode_section_setup, reconstruct_section},
    replay_speed::ReplaySpeed,
};
use beatkernel_platform::audio::presentation::validation::{
    NativePresentationValidator, OriginalNativePresentationEvidence,
};
use std::{cell::Cell, collections::VecDeque, rc::Rc};

fn p(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    p(1, 10_000_000_000 + ns)
}
fn raw(ns: i64) -> ClockPoint {
    p(2, 1_000_000_000 + ns)
}
fn logical(ns: i64) -> ClockPoint {
    p(3, 5_000_000_000 + ns)
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(1_048_576, 8192, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}

#[derive(Default)]
struct SoloObserver {
    header: Option<ReplayHeader>,
    reports: usize,
    completed: usize,
}
impl SoloCompetitionPort for SoloObserver {
    fn expected_policy_header(&self) -> Option<&ReplayHeader> {
        self.header.as_ref()
    }
    fn observe(&mut self, _: &RuntimeReport) -> NativeGameplayResult<()> {
        self.reports += 1;
        Ok(())
    }
    fn mark_native_completed(&mut self) {
        self.completed += 1;
    }
}
#[derive(Default)]
struct GroupObserver {
    headers: Vec<(PlayerId, ReplayHeader)>,
    rows: Vec<Vec<PlayerId>>,
    completed: usize,
}
impl GroupCompetitionPort for GroupObserver {
    fn expected_policy_header(&self, player: PlayerId) -> Option<&ReplayHeader> {
        self.headers
            .iter()
            .find(|(id, _)| *id == player)
            .map(|(_, h)| h)
    }
    fn observe(&mut self, members: &[MemberProgress]) -> NativeGameplayResult<()> {
        self.rows.push(members.iter().map(|m| m.player).collect());
        Ok(())
    }
    fn mark_native_completed(&mut self) {
        self.completed += 1;
    }
}
#[derive(Default)]
struct Control {
    waits: usize,
    reads: usize,
}
impl NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        self.reads += 1;
        Ok(self.reads as u64)
    }
    fn checked_add(m: u64, d: std::time::Duration) -> Option<u64> {
        m.checked_add(d.as_secs())
    }
    fn wait(&mut self, _: std::time::Duration) -> NativeGameplayResult<()> {
        self.waits += 1;
        Ok(())
    }
}
struct Host {
    cancel: Rc<Cell<bool>>,
    reports: Vec<(PlayerId, RuntimeReport)>,
    ends: Vec<Timestamp>,
    completions: usize,
    desired_pause: Rc<Cell<bool>>,
    pause_states: Vec<PauseState>,
}
impl NativeGameplayHost for Host {
    fn cancelled(&self) -> bool {
        self.cancel.get()
    }
    fn pause_requested(&self) -> bool {
        self.desired_pause.get()
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, state: PauseState) {
        self.pause_states.push(state);
    }
    fn publish_section_end(&mut self, end: Timestamp) {
        self.ends.push(end);
    }
    fn publish_report(&mut self, _: &RuntimeReport) -> NativeGameplayResult<()> {
        panic!("cohort must publish local reports")
    }
    fn publish_local_reports(&mut self, reports: &[PlayerReport]) -> NativeGameplayResult<()> {
        self.reports
            .extend(reports.iter().map(|r| (r.player, r.report.clone())));
        Ok(())
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
    fn publish_completed_local(
        &mut self,
        results: &[(PlayerId, CompletedPlayResult)],
    ) -> NativeGameplayResult<()> {
        self.completions += results.len();
        Ok(())
    }
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
                TerminalMutation::SongDrift => forged.song_position = Timestamp::from_nanos(1),
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
    basis: beatkernel::audio::OutputFrameBasis,
    report: Option<RenderReport>,
    step: usize,
    input_frame: Option<usize>,
    members: usize,
    cancel: Rc<Cell<bool>>,
    pcm: Vec<f32>,
    missing_cut: bool,
    missing_observation: bool,
    stationary: bool,
    interval: bool,
    pause_flow: bool,
    desired_pause: Rc<Cell<bool>>,
    block_frames: usize,
    delayed_inputs: Option<(usize, i64, i64)>,
    prefix_cap: Option<(usize, i64)>,
    renders: Vec<RenderReport>,
    terminal_race: bool,
    race_armed: bool,
    race_reports: Vec<RenderReport>,
    shifted_origin: bool,
    terminal_script: Option<TerminalScript>,
}
impl GameplayDevice for Device {
    type Presentation = PresentationEstimator;
    fn publish_paused_audio_output(
        &mut self,
        _: beatkernel_bms_runtime::gameplay_presentation::GameplayAudioOutputContext<'_>,
        _: ClockPoint,
    ) -> NativeGameplayResult<bool> {
        Ok(false)
    }
    fn observe(&mut self, _: &mut Self::Presentation) -> NativeGameplayResult<()> {
        panic!("live audio owner must not call legacy presentation")
    }
    fn observe_audio(
        &mut self,
        presentation: &mut NativeAudioPresentation,
    ) -> NativeGameplayResult<()> {
        self.step += 1;
        if self.step > 80 {
            return Err("bounded independent software fixture exceeded 80 callbacks".into());
        }
        let mut block = vec![0.0; self.block_frames];
        self.report = Some(self.mixer.render(&mut block)?);
        self.renders.push(self.report.unwrap());
        self.pcm.extend(block);
        if self.step == 75 {
            self.cancel.set(true);
        }
        if !self.missing_observation {
            // First pair precedes every possible finite fence. Later pairs
            // report the prior callback frame, independently of render-ahead.
            let frame = if self.stationary {
                0
            } else {
                self.step * self.block_frames - 1
            };
            presentation.admit(NativeAudioSnapshot {
                epoch: 0,
                basis: self.basis,
                evidence: if self.interval {
                    OriginalNativePresentationEvidence::Asio {
                        observation: beatkernel_platform::audio::asio::AsioPresentationObservation::from_render(
                            self.report.unwrap(), 1000,
                            beatkernel_platform::audio::asio::MultimediaHostInterval {
                                before: host(frame as i64 * 1_000_000 - 100),
                                after: host(frame as i64 * 1_000_000 + 100),
                            }, 0, 0, raw(0))?,
                        basis: Some(self.basis),
                    }
                } else { OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                    source: raw(frame as i64 * 1_000_000),
                    target: host(frame as i64 * 1_000_000 + if self.shifted_origin {10_000_000} else {0}),
                }) },
            })?;
        }
        Ok(())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        if self.race_armed {
            self.race_armed = false;
            let prior = self.report.expect("actual prior callback");
            self.race_reports.push(prior);
            let mut pcm = vec![0.0; self.block_frames];
            let crossing = self.mixer.render(&mut pcm)?;
            self.pcm.extend(pcm);
            self.renders.push(crossing);
            self.race_reports.push(crossing);
            self.report = Some(crossing);
            return Ok(Some(prior));
        }
        Ok(match self.terminal_script.as_mut() {
            Some(script) => script.read(self.report),
            None => self.report,
        })
    }
    fn audio_pause_observation(
        &mut self,
        presentation: &NativeAudioPresentation,
        _: ClockPoint,
    ) -> NativeGameplayResult<beatkernel_bms_runtime::live_pause::LivePauseObservation> {
        let record = presentation
            .latest_record()
            .ok_or("no actual software presentation")?;
        if let OriginalNativePresentationEvidence::Asio { observation, .. } = *record.evidence() {
            Ok(
                beatkernel_bms_runtime::live_pause::LivePauseObservation::Interval {
                    observation: Some(
                        beatkernel_bms_runtime::playback_pause::PauseIntervalObservation {
                            output_origin: observation.output_origin,
                            sample_rate: observation.sample_rate,
                            render: observation.render,
                            clock: beatkernel_bms_runtime::native_start::StartInterval::new(
                                observation.output,
                                observation.host.before,
                                observation.host.after,
                            )?,
                        },
                    ),
                    now: self.host_now()?,
                },
            )
        } else {
            Ok(beatkernel_bms_runtime::live_pause::LivePauseObservation::Point(record.pair()))
        }
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(host(
            (self.step * self.block_frames) as i64 * 1_000_000
                + if self.shifted_origin { 10_000_000 } else { 0 },
        ))
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        if self.shifted_origin && self.step == 2 {
            for member in (0..self.members).rev() {
                events.push_back(PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(101 + member as u64), host(9_000_000), 0),
                    control: PhysicalControlId::keyboard(7),
                    state: ButtonState::Down,
                }));
            }
        }
        if let Some((delivery_step, down_ns, up_ns)) = self.delayed_inputs {
            if self.step == delivery_step {
                self.race_armed = self.terminal_race;
                for member in (0..self.members).rev() {
                    for (sequence, ns, state) in
                        [(0, down_ns, ButtonState::Down), (1, up_ns, ButtonState::Up)]
                    {
                        events.push_back(PhysicalInputEvent::Button(ButtonEvent {
                            meta: EventMeta::new(DeviceId(101 + member as u64), host(ns), sequence),
                            control: PhysicalControlId::keyboard(7),
                            state,
                        }));
                    }
                }
            }
        }
        if self.pause_flow {
            match self.step {
                4 | 14 => self.desired_pause.set(true),
                9 | 19 => self.desired_pause.set(false),
                _ => {}
            }
        }
        if self.input_frame == Some(self.step) || self.input_frame.map(|f| f + 1) == Some(self.step)
        {
            let down = self.input_frame == Some(self.step);
            // Deliberately reverse collector arrival; InputMerger owns ordering.
            for member in (0..self.members).rev() {
                events.push_back(PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(
                        DeviceId(101 + member as u64),
                        self.host_now()?,
                        u64::from(!down) + u64::from(self.shifted_origin),
                    ),
                    control: PhysicalControlId::keyboard(7),
                    state: if down {
                        ButtonState::Down
                    } else {
                        ButtonState::Up
                    },
                }));
            }
        }
        Ok(InputBatch {
            completed_through: if self.missing_cut
                || self
                    .terminal_script
                    .as_ref()
                    .is_some_and(TerminalScript::incomplete)
            {
                None
            } else if let Some((through_step, cut_ns)) =
                self.prefix_cap.filter(|(step, _)| self.step <= *step)
            {
                let _ = through_step;
                Some(host(
                    cut_ns.min((self.step * self.block_frames) as i64 * 1_000_000),
                ))
            } else {
                Some(self.host_now()?)
            },
            backlog: self.input_frame == Some(self.step),
            closed: false,
        })
    }
    fn observe_audio_end(
        &mut self,
        end: &mut NativeEnd,
        presentation: &NativeAudioPresentation,
        rendered: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        if let Some(script) = self.terminal_script.as_mut() {
            if script.marker.is_some() && rendered == script.marker {
                script.owner_marker_seen = true;
            }
        }
        match presentation.latest_record() {
            Some(record) => Ok(end.observe(rendered, record.pair())?),
            None => Ok(None),
        }
    }
    fn observe_end(
        &mut self,
        _: &mut NativeEnd,
        _: &Self::Presentation,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        panic!("legacy end")
    }
    fn seed_resume(
        &mut self,
        _: &mut Self::Presentation,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        panic!("legacy resume")
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        panic!("raw report owns schedule")
    }
}

struct Fixture {
    source: beatkernel_bms::BmsChart,
    setup: LiveSpeedSetup,
    config: AudioGameplayConfig,
    device: Device,
    group: RuntimeGroup,
    states: Vec<GameplayPlayerState<SoloObserver>>,
    network: GroupObserver,
    merger: InputMerger,
    bgm: BgmFeeder,
    presentation: NativeAudioPresentation,
    pause: NativePause,
    end: Option<NativeEnd>,
    delivery: InputDeliveryTelemetry,
    pre: u64,
    control: Control,
    host: Host,
}
impl Fixture {
    fn new(
        members: usize,
        fraction: &str,
        preroll_ns: i64,
        end_ns: Option<i64>,
        input_frame: Option<usize>,
    ) -> Self {
        let speed: ReplaySpeed = fraction.parse().unwrap();
        let setup = LiveSpeedSetup::new(
            Timestamp::ZERO,
            Duration::from_nanos(preroll_ns),
            speed,
            end_ns.map(Timestamp::from_nanos),
        )
        .unwrap();
        let source = beatkernel_bms::parse(
            "#BPM 5000\n#WAV01 key.wav\n#00011:00010000\n#10012:01",
            Default::default(),
        )
        .unwrap();
        let format = AudioFormat::new(1000, 1).unwrap();
        let pcm_limits = PcmLimits::new(64, 256, 1).unwrap();
        let mut bank = SampleBank::new(format, pcm_limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.005; 8], pcm_limits).unwrap(),
        )
        .unwrap();
        let (producer, consumer) = command_queue(256).unwrap();
        let finite = setup.playback_end_frame(1000).unwrap();
        let prefix = if finite != Some(0) && speed.rate() != Rate::NORMAL {
            vec![AudioCommand::SetRate {
                rate: speed.rate(),
                at: raw(0).timestamp,
            }]
        } else {
            vec![]
        };
        let audio_limits = AudioLimits::new(256, 128, 256, 256, 256).unwrap();
        let mut mixer_config =
            MixerConfig::new(format, ClockDomainId(2), raw(0).timestamp, audio_limits);
        if let Some(frame) = finite {
            mixer_config = mixer_config.with_playback_end_frame(frame);
        }
        let mixer = Mixer::new(mixer_config, bank, consumer).unwrap();
        let basis = mixer.output_frame_basis();
        let mut configs = Vec::new();
        let mut states = Vec::new();
        let mut network = GroupObserver::default();
        for member in 0..members {
            let player = PlayerId(member as u32 + 7);
            let device = DeviceId(member as u64 + 101);
            let judge = JudgeEngine::new(
                source.compile().unwrap().chart,
                source.rules(),
                beatkernel_bms_runtime::play_policy::ResolvedPlayPolicy::builtin(0, 0, 0)
                    .unwrap()
                    .judge()
                    .clone(),
            )
            .unwrap();
            let capture = LiveReplayCapture::new_with_gauge_and_speed(
                &judge,
                ClockDomainId(3),
                limits(),
                Timestamp::ZERO,
                0,
                setup.end(),
                beatkernel_bms::BmsInputMode::ButtonOnly,
                None,
                &GaugeProfile::default(),
                speed,
            )
            .unwrap();
            let header = capture.header().clone();
            network.headers.push((player, header.clone()));
            configs.push(MemberConfig {
                player,
                device: Some(device),
                bindings: BindingMap::from_bindings([Binding {
                    device: DeviceSelector::Exact(device),
                    physical: PhysicalControlId::keyboard(7),
                    game_control: GameControlId(0x11),
                }])
                .unwrap(),
                sounds: vec![SoundBinding {
                    object: source.compile().unwrap().chart.objects()[0].id,
                    stage: JudgeStage::Instant,
                    sample: SampleId(1),
                    voice: VoiceId(member as u64 + 1),
                    gain: 1.0,
                }],
                judge,
            });
            states.push(GameplayPlayerState {
                player,
                capture: Some(capture),
                competition: Some(SoloObserver {
                    header: Some(header),
                    ..Default::default()
                }),
                completion: None,
                score: ScoreSummary::default(),
                gauge: BmsGauge::default(),
                last_song: setup.song_origin(),
            });
        }
        let mut group = RuntimeGroup::new(
            ClockDomainId(3),
            ClockDomainId(2),
            setup.transport(logical(0).timestamp).unwrap(),
            producer,
            configs,
            128,
            &[],
        )
        .unwrap();
        group.set_processing_clock(RuntimeProcessingClock::Disabled);
        if let Some(end) = setup.end() {
            group.set_song_end(end).unwrap();
        }
        let mut pause = NativePause::new(raw(0), ClockDomainId(1), 1000).unwrap();
        if let Some(frame) = finite {
            pause = pause.with_playback_end_frame(frame).unwrap();
        }
        let presentation = NativeAudioPresentation::new(
            AudioAuthority::new(
                AudioAuthorityConfig {
                    history_capacity: 128,
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
        .unwrap();
        let cancel = Rc::new(Cell::new(false));
        let desired_pause = Rc::new(Cell::new(false));
        let mut bgm = BgmFeeder::from_output_commands_with_rate(
            prefix,
            BgmConfig {
                output_origin: raw(0),
                sample_rate: 1000,
                preroll: Duration::ZERO,
                lookahead: Duration::from_nanos(10_000_000),
                max_pending: 128,
            },
            speed.rate(),
        )
        .unwrap();
        // Cold preparation primes origin work through the same public group
        // producer and feeder credit port before any actual output callback.
        // No mixer execution, presentation or judgment is granted here.
        let primed = bgm
            .feed(0, 256, |command| group.enqueue_audio(command))
            .unwrap();
        assert_eq!(
            primed.total_admitted,
            usize::from(finite != Some(0) && speed.rate() != Rate::NORMAL)
        );
        Self {
            source,
            setup,
            config: AudioGameplayConfig {
                section_start: Timestamp::ZERO,
                gameplay: NativeGameplayConfig {
                    origin: host(0),
                    stream_origin: raw(0),
                    playback_origin: raw(0),
                    song_origin: setup.song_origin(),
                    sample_rate: 1000,
                    end_song: setup.end(),
                    advance_lag: Duration::ZERO,
                    seconds: None,
                    pause_supported: false,
                    logical_schedule: true,
                },
            },
            device: Device {
                mixer,
                basis,
                report: None,
                step: 0,
                input_frame,
                members,
                cancel: cancel.clone(),
                pcm: vec![],
                missing_cut: false,
                missing_observation: false,
                stationary: false,
                interval: false,
                pause_flow: false,
                desired_pause: desired_pause.clone(),
                block_frames: 1,
                delayed_inputs: None,
                prefix_cap: None,
                renders: vec![],
                terminal_race: false,
                race_armed: false,
                race_reports: vec![],
                shifted_origin: false,
                terminal_script: None,
            },
            group,
            states,
            network,
            merger: InputMerger::new(
                ClockDomainId(1),
                host(0),
                (0..members).map(|i| DeviceId(101 + i as u64)).collect(),
                256,
            )
            .unwrap(),
            bgm,
            presentation,
            pause,
            end: finite.map(|frame| NativeEnd::new(raw(0), ClockDomainId(1), 1000, frame).unwrap()),
            delivery: InputDeliveryTelemetry::new(256, ClockDomainId(1)).unwrap(),
            pre: 0,
            control: Control::default(),
            host: Host {
                cancel,
                reports: vec![],
                ends: vec![],
                completions: 0,
                desired_pause,
                pause_states: vec![],
            },
        }
    }
    fn run(&mut self) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
        run_cohort_audio_with_ports_and_speed(
            &mut self.device,
            AudioCohortSession {
                group: &mut self.group,
                network: Some(&mut self.network),
                states: &mut self.states,
                merger: &mut self.merger,
                bgm: &mut self.bgm,
                discipline: &mut self.presentation,
                pause: &mut self.pause,
                end: &mut self.end,
                delivery: &mut self.delivery,
                pre_origin_inputs: &mut self.pre,
            },
            self.config,
            &mut self.control,
            &mut self.host,
            self.setup,
        )
    }
    fn assert_cold(&self) {
        assert_eq!(self.device.step, 0);
        assert_eq!(self.device.mixer.frame_cursor(), 0);
        assert_eq!(self.control.waits, 0);
        assert_eq!(self.control.reads, 0);
        assert!(self.host.reports.is_empty());
        assert!(self.network.rows.is_empty());
        for state in &self.states {
            assert_eq!(state.score, ScoreSummary::default());
            assert!(state.capture.as_ref().unwrap().records().is_empty());
            assert_eq!(
                self.group
                    .member_judge(state.player)
                    .unwrap()
                    .effective_song_time(),
                None
            );
        }
    }
}

#[test]
fn all_four_speeds_preserve_original_member_judgments_provenance_and_raw_keysounds() {
    for (speed, frame) in [("1/2", 27), ("1/1", 15), ("2/1", 9), ("3/2", 11)] {
        for members in [2, 4, 64] {
            let mut f = Fixture::new(members, speed, 3_000_000, None, Some(frame));
            assert!(f.run().unwrap().is_none());
            assert_eq!(f.merger.pending(), 0);
            assert_eq!(f.delivery.observed_events(), members as u64 * 2);
            assert_eq!(f.network.completed, 0);
            assert_eq!(f.host.completions, 0);
            let first_input_members: Vec<_> = f
                .host
                .reports
                .iter()
                .filter(|(_, r)| r.input.is_some())
                .take(members)
                .map(|(player, _)| *player)
                .collect();
            assert_eq!(
                first_input_members,
                (0..members)
                    .map(|i| PlayerId(i as u32 + 7))
                    .collect::<Vec<_>>()
            );
            for (member, state) in f.states.iter().enumerate() {
                assert_eq!(state.score.hits, 1);
                assert_eq!(state.score.misses, 0);
                let capture = state.capture.as_ref().unwrap();
                let setup = decode_section_setup(&capture.header().options).unwrap();
                assert_eq!(setup.source_speed, speed.parse::<ReplaySpeed>().unwrap());
                let inputs: Vec<_> = capture
                    .records()
                    .iter()
                    .filter_map(|record| match &record.operation {
                        ReplayOperation::Input(input) => Some((record, input)),
                        _ => None,
                    })
                    .collect();
                assert_eq!(inputs.len(), 2);
                assert_eq!(inputs[0].0.song_time, Timestamp::from_nanos(12_000_000));
                assert_eq!(
                    inputs[0].1.physical.meta().source,
                    DeviceId(101 + member as u64)
                );
                assert_eq!(
                    inputs[0].1.physical.meta().original_clock_point,
                    Some(host(frame as i64 * 1_000_000))
                );
                assert_eq!(
                    inputs[0].1.physical.meta().timestamp,
                    logical(frame as i64 * 1_000_000).timestamp
                );
                assert_eq!(inputs[0].1.physical.meta().sequence, 0);
                assert_eq!(inputs[1].1.physical.meta().sequence, 1);
                let file = beatkernel::replay::codec::ReplayFile::new(
                    capture.header().clone(),
                    capture.records().to_vec(),
                );
                let replay = reconstruct_section(&f.source, file, limits()).unwrap();
                assert_eq!(
                    replay.engine().stable_hash().unwrap(),
                    f.group
                        .member_judge(state.player)
                        .unwrap()
                        .stable_hash()
                        .unwrap()
                );
                let hit = f
                    .host
                    .reports
                    .iter()
                    .find(|(p, r)| *p == state.player && !r.audio_commands.is_empty())
                    .unwrap();
                for command in &hit.1.audio_commands {
                    if let AudioCommand::Play { at, .. } = command {
                        assert_eq!(*at, hit.1.audio_at.timestamp);
                    }
                }
                assert_eq!(hit.1.audio_at.domain, ClockDomainId(2));
            }
            assert!(f.device.pcm.iter().any(|sample| *sample > 0.0));
            assert!(f.network.rows.iter().all(|row| *row
                == (0..members)
                    .map(|i| PlayerId(i as u32 + 7))
                    .collect::<Vec<_>>()));
        }
    }
}

#[test]
fn actual_native_two_and_four_member_pumps_discard_negative_origin_before_key_state() {
    for count in [2, 4] {
        let mut f = Fixture::new(count, "3/2", 3_000_000, None, Some(11));
        f.presentation = NativeAudioPresentation::new(
            AudioAuthority::new(
                AudioAuthorityConfig {
                    history_capacity: 128,
                    max_observation_age: Duration::from_nanos(1_000_000_000),
                    input_extrapolation: ExtrapolationPolicy::Bounded {
                        before: Duration::from_nanos(20_000_000),
                        after: Duration::ZERO,
                    },
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
        .unwrap();
        f.device.shifted_origin = true;
        assert!(f.run().unwrap().is_none());
        assert_eq!(f.pre, count as u64);
        assert_eq!(f.merger.pending(), 0);
        let inputs: Vec<_> = f
            .host
            .reports
            .iter()
            .filter(|(_, r)| r.input.is_some())
            .collect();
        assert_eq!(inputs.len(), count * 2);
        for (_, report) in inputs {
            let meta = report.input.as_ref().unwrap().meta();
            assert!(meta.sequence == 1 || meta.sequence == 2);
            assert!(meta.original_clock_point.unwrap().timestamp >= host(21_000_000).timestamp);
            assert_eq!(report.audio_at.domain, ClockDomainId(2));
        }
        for state in &f.states {
            assert_eq!(
                state.score.hits, 1,
                "discarded Down cannot block the first genuine Down"
            );
            let inputs: Vec<_> = state
                .capture
                .as_ref()
                .unwrap()
                .records()
                .iter()
                .filter_map(|r| match &r.operation {
                    ReplayOperation::Input(saved) => Some(saved.physical.meta().sequence),
                    _ => None,
                })
                .collect();
            assert_eq!(inputs, vec![1, 2]);
        }
    }
}

#[test]
fn actual_finite_output_finalizes_every_member_once_at_original_end() {
    for (speed, frames) in [("1/2", 51), ("1/1", 27), ("2/1", 15), ("3/2", 19)] {
        let mut f = Fixture::new(4, speed, 3_000_000, Some(24_000_000), None);
        let result = f
            .run()
            .unwrap()
            .expect("genuine finite output must complete cohort");
        assert_eq!(result.len(), 4);
        assert_eq!(f.network.completed, 1);
        assert_eq!(
            f.device.report.unwrap().playback_end_physical_frame,
            Some(frames)
        );
        assert_eq!(f.host.ends, [Timestamp::from_nanos(24_000_000)]);
        for state in &f.states {
            assert_eq!(state.last_song, Timestamp::from_nanos(24_000_000));
            assert_eq!(state.competition.as_ref().unwrap().completed, 1);
            assert_eq!(
                state
                    .capture
                    .as_ref()
                    .unwrap()
                    .records()
                    .last()
                    .unwrap()
                    .song_time,
                state.last_song
            );
            assert!(matches!(
                state
                    .capture
                    .as_ref()
                    .unwrap()
                    .records()
                    .last()
                    .unwrap()
                    .operation,
                ReplayOperation::Advance
            ));
        }
    }
}

#[test]
fn zero_and_quantized_finite_fences_need_real_presentation_and_all_member_advances() {
    // Positive original spans may quantize to zero output; original record
    // metadata still retains its established strictly positive section span.
    for (end, fraction) in [(1, "2/1"), (1, "3/2"), (3, "2/1")] {
        let mut f = Fixture::new(2, fraction, 0, Some(end), None);
        assert!(f.run().unwrap().is_some());
        for state in &f.states {
            assert_eq!(state.last_song, Timestamp::from_nanos(end));
            assert_eq!(
                state
                    .capture
                    .as_ref()
                    .unwrap()
                    .records()
                    .last()
                    .unwrap()
                    .song_time,
                Timestamp::from_nanos(end)
            );
        }
        let mut missing = Fixture::new(2, fraction, 0, Some(end), None);
        missing.device.missing_observation = true;
        assert!(missing.run().unwrap().is_none());
        assert_eq!(missing.network.completed, 0);
        assert!(missing.host.ends.is_empty());
        assert!(missing.states.iter().all(|s| s.completion.is_none()));
    }
}

#[test]
fn rendered_fence_without_acquisition_cut_does_not_complete_any_member() {
    let mut f = Fixture::new(4, "2/1", 3_000_000, Some(24_000_000), Some(9));
    f.device.missing_cut = true;
    assert!(f.run().unwrap().is_none());
    assert!(f
        .device
        .report
        .unwrap()
        .playback_end_physical_frame
        .is_some());
    assert_eq!(f.merger.pending(), 8);
    assert_eq!(f.network.completed, 0);
    assert!(f
        .states
        .iter()
        .all(|s| s.capture.as_ref().unwrap().records().is_empty()));
}

#[test]
fn cold_start_preroll_rate_end_and_wrong_integer_only_phase_refuse_before_effects() {
    for variant in 0..6 {
        let mut f = Fixture::new(2, "3/2", 3, Some(24_000_000), None);
        match variant {
            0 => f.config.section_start = Timestamp::from_nanos(1),
            1 => {
                f.setup = LiveSpeedSetup::new(
                    Timestamp::ZERO,
                    Duration::from_nanos(4),
                    "3/2".parse().unwrap(),
                    f.setup.end(),
                )
                .unwrap()
            }
            2 => {
                f.setup = LiveSpeedSetup::new(
                    Timestamp::ZERO,
                    Duration::from_nanos(3),
                    "2/1".parse().unwrap(),
                    f.setup.end(),
                )
                .unwrap()
            }
            3 => {
                f.setup = LiveSpeedSetup::new(
                    Timestamp::ZERO,
                    Duration::from_nanos(3),
                    "3/2".parse().unwrap(),
                    Some(Timestamp::from_nanos(24_000_001)),
                )
                .unwrap()
            }
            4 => {
                *f.group.transport_mut() = Transport::new(
                    logical(0).timestamp,
                    f.setup.song_origin(),
                    Rate::new(3, 2).unwrap(),
                )
            }
            _ => f.config.gameplay.playback_origin = raw(1),
        }
        assert!(f.run().is_err(), "cold mismatch {variant} accepted");
        f.assert_cold();
    }
}

#[test]
fn last_member_wrong_recorded_speed_refuses_whole_cohort_cold() {
    let mut f = Fixture::new(4, "3/2", 3_000_000, None, None);
    let wrong = Fixture::new(4, "2/1", 3_000_000, None, None);
    f.states[3].capture = wrong.states.into_iter().nth(3).unwrap().capture;
    assert!(f.run().is_err());
    f.assert_cold();
}

#[test]
fn wrong_shared_competition_condition_refuses_before_member_publication() {
    let mut f = Fixture::new(4, "3/2", 3_000_000, None, None);
    let wrong = Fixture::new(4, "2/1", 3_000_000, None, None);
    f.network.headers[3].1 = wrong.network.headers[3].1.clone();
    assert!(f.run().is_err());
    f.assert_cold();
}

#[test]
fn cancelled_default_cohort_leaves_original_owners_and_capture_unplayed() {
    let mut f = Fixture::new(2, "1/1", 3_000_000, None, None);
    f.host.cancel.set(true);
    assert!(f.run().unwrap().is_none());
    f.assert_cold();
    assert_eq!(f.network.completed, 0);
}

#[test]
fn repeated_actual_source_pause_preserves_fractional_phase_for_every_member_transport() {
    for (fraction, preroll, next_ns_delta) in [("1/2", 1, 1), ("3/2", 3, 2)] {
        let mut f = Fixture::new(4, fraction, preroll, None, None);
        f.config.gameplay.pause_supported = true;
        f.device.pause_flow = true;
        assert!(f.run().unwrap().is_none());
        assert!(
            f.host
                .pause_states
                .iter()
                .filter(|s| **s == PauseState::Paused)
                .count()
                >= 2
        );
        let anchors = f.group.transport().anchors();
        let resumed: Vec<_> = anchors
            .windows(2)
            .filter(|pair| {
                pair[0].rate == Rate::ZERO
                    && pair[1].rate == fraction.parse::<ReplaySpeed>().unwrap().rate()
            })
            .collect();
        assert_eq!(resumed.len(), 2);
        for pair in resumed {
            // Whole-millisecond output E is even. Removing odd P leaves the
            // literal half-nanosecond phase: next +1ns advances 1ns or 2ns.
            let resume = pair[1];
            assert_eq!(
                f.group
                    .transport()
                    .position_at(Timestamp::from_nanos(resume.host_time.as_nanos() + 1))
                    .unwrap(),
                Timestamp::from_nanos(resume.song_time.as_nanos() + next_ns_delta)
            );
        }
        assert_eq!(f.pause.epoch(), 0);
        assert_eq!(f.network.completed, 0);
    }
}

#[test]
fn software_render_ahead_without_advancing_native_presentation_is_not_completion() {
    let mut f = Fixture::new(2, "2/1", 3_000_000, Some(24_000_000), None);
    f.device.stationary = true;
    assert!(f.run().unwrap().is_none());
    assert!(f
        .device
        .report
        .unwrap()
        .playback_end_physical_frame
        .is_some());
    assert!(f.host.ends.is_empty());
    assert_eq!(f.network.completed, 0);
    assert!(f.states.iter().all(|s| s.completion.is_none()));
}

#[test]
fn original_asio_interval_pause_retains_odd_phase_through_two_cohort_resumes() {
    for (fraction, preroll, delta) in [("1/2", 1, 1), ("3/2", 3, 2)] {
        let mut f = Fixture::new(4, fraction, preroll, None, None);
        f.config.gameplay.pause_supported = true;
        f.device.pause_flow = true;
        f.device.interval = true;
        assert!(f.run().unwrap().is_none());
        assert!(matches!(
            f.presentation.latest_record().unwrap().evidence(),
            OriginalNativePresentationEvidence::Asio { .. }
        ));
        assert_eq!(
            f.host
                .pause_states
                .iter()
                .filter(|s| **s == PauseState::Paused)
                .count(),
            2
        );
        let resumed: Vec<_> = f
            .group
            .transport()
            .anchors()
            .windows(2)
            .filter(|pair| pair[0].rate == Rate::ZERO && pair[1].rate == f.setup.speed().rate())
            .collect();
        assert_eq!(resumed.len(), 2);
        for pair in resumed {
            assert_eq!(
                f.group
                    .transport()
                    .position_at(Timestamp::from_nanos(pair[1].host_time.as_nanos() + 1))
                    .unwrap(),
                Timestamp::from_nanos(pair[1].song_time.as_nanos() + delta)
            );
        }
        assert_eq!(f.network.completed, 0);
    }
}

/// An independently scripted native target counter over the actual converted
/// owner. It intentionally trails submission by two milliseconds; source render
/// adoption and a cached converted block cannot authorize target pause ACK.
struct TargetDevice {
    output: beatkernel_platform::audio::ConvertedNativeOutputState,
    basis: beatkernel::audio::TargetFrameBasis,
    step: usize,
    held: bool,
    cancel: Rc<Cell<bool>>,
    desired_pause: Rc<Cell<bool>>,
    early_pause: usize,
    early_resume: usize,
    holds: Vec<bool>,
    target_frames: usize,
    presentation_delay_steps: usize,
    pause_flow: bool,
    members: usize,
    delayed_inputs: Option<(usize, i64, i64)>,
    rendered: Vec<RenderReport>,
}
impl TargetDevice {
    fn pair(&self) -> ClockPair {
        let played = self.step.saturating_sub(self.presentation_delay_steps) as u64
            * self.target_frames as u64;
        ClockPair {
            source: self.basis.point_at_stream_frame(played).unwrap(),
            target: host(played as i64 * 500_000),
        }
    }
}
impl GameplayDevice for TargetDevice {
    type Presentation = PresentationEstimator;
    fn publish_paused_audio_output(
        &mut self,
        _: beatkernel_bms_runtime::gameplay_presentation::GameplayAudioOutputContext<'_>,
        _: ClockPoint,
    ) -> NativeGameplayResult<bool> {
        Ok(false)
    }
    fn observe(&mut self, _: &mut Self::Presentation) -> NativeGameplayResult<()> {
        panic!("target owner cannot use legacy estimator")
    }
    fn observe_audio(
        &mut self,
        presentation: &mut NativeAudioPresentation,
    ) -> NativeGameplayResult<()> {
        self.step += 1;
        if self.step > 80 {
            return Err("independent target fixture callback bound".into());
        }
        if self.held {
            let before = (
                self.output.mixer().frame_cursor(),
                self.output.converter_owner().source_position(),
            );
            self.output.render_held_pending(self.target_frames)?;
            assert_eq!(
                before,
                (
                    self.output.mixer().frame_cursor(),
                    self.output.converter_owner().source_position()
                ),
                "held target output must not advance the source read head"
            );
        } else {
            self.output.render_pending(self.target_frames)?;
        }
        self.output.admit(self.target_frames)?;
        if let Some(report) = self.output.last_real_source_report() {
            self.rendered.push(report);
        }
        let pair = self.pair();
        let facts = self.output.boundaries();
        if let Some(boundary) = facts.pause {
            if self.output.mixer().is_paused()
                && !self.held
                && pair.source.timestamp
                    < boundary.target_time.point(self.basis.origin())?.timestamp
            {
                self.early_pause += 1;
            }
        }
        if let Some(boundary) = facts.resume {
            if !self.output.mixer().is_paused()
                && pair.source.timestamp
                    < boundary.target_time.point(self.basis.origin())?.timestamp
            {
                self.early_resume += 1;
            }
        }
        presentation.admit_target(
            beatkernel_bms_runtime::native_audio_presentation::TargetNativeAudioSnapshot {
                epoch: 0,
                basis: self.basis,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(pair),
            },
        )?;
        if self.step == 75 {
            self.cancel.set(true);
        }
        Ok(())
    }
    fn audio_pause_observation(
        &mut self,
        presentation: &NativeAudioPresentation,
        _: ClockPoint,
    ) -> NativeGameplayResult<beatkernel_bms_runtime::live_pause::LivePauseObservation> {
        Ok(
            beatkernel_bms_runtime::live_pause::LivePauseObservation::Target {
                epoch: 0,
                basis: self.basis,
                facts: self.output.boundaries(),
                source: self.output.last_real_source_report(),
                pair: presentation
                    .latest_record()
                    .ok_or("target observation missing")?
                    .pair(),
            },
        )
    }
    fn set_audio_held(&mut self, held: bool) -> NativeGameplayResult<()> {
        assert!(
            self.output.mixer().pause_requested(),
            "native held effect must precede producer resume"
        );
        if held {
            let boundary = self
                .output
                .boundaries()
                .pause
                .ok_or("actual converted pause fact missing")?;
            assert!(
                self.pair().source.timestamp
                    >= boundary.target_time.point(self.basis.origin())?.timestamp,
                "source pause alone cannot acknowledge target pause"
            );
        }
        self.held = held;
        self.holds.push(held);
        Ok(())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(self.output.last_real_source_report())
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(self.pair().target)
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        if self.pause_flow {
            match self.step {
                4 | 14 => self.desired_pause.set(true),
                9 | 19 => self.desired_pause.set(false),
                _ => {}
            }
        }
        if let Some((delivery_step, down_ns, up_ns)) = self.delayed_inputs {
            if self.step == delivery_step {
                for member in (0..self.members).rev() {
                    for (sequence, ns, state) in
                        [(0, down_ns, ButtonState::Down), (1, up_ns, ButtonState::Up)]
                    {
                        events.push_back(PhysicalInputEvent::Button(ButtonEvent {
                            meta: EventMeta::new(DeviceId(101 + member as u64), host(ns), sequence),
                            control: PhysicalControlId::keyboard(7),
                            state,
                        }));
                    }
                }
            }
        }
        Ok(InputBatch {
            completed_through: Some(self.pair().target),
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
        panic!("legacy target end")
    }
    fn seed_resume(
        &mut self,
        _: &mut Self::Presentation,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        panic!("legacy target resume")
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        panic!("actual source render report owns raw schedule")
    }
}

#[test]
fn actual_converted_target_cohort_pause_waits_for_target_crossing_and_preserves_phase() {
    for (fraction, preroll, delta) in [("1/2", 1, 1), ("3/2", 3, 2)] {
        let mut f = Fixture::new(2, fraction, preroll, None, None);
        f.config.gameplay.pause_supported = true;
        let output = beatkernel_platform::audio::ConvertedNativeOutputState::new(
            f.device.mixer,
            beatkernel_platform::audio::DeviceFormat::new(
                2000,
                1,
                beatkernel_platform::audio::SampleEncoding::Float32,
                None,
            )
            .unwrap(),
            beatkernel::audio::ChannelMatrix::default_mix(1, 1).unwrap(),
            beatkernel::audio::ResampleQuality::Linear,
            16,
        )
        .unwrap_or_else(|_| panic!("valid converted software output"));
        let basis = output.target_frame_basis();
        f.presentation =
            beatkernel_bms_runtime::native_audio_startup::new_target_audio_presentation(
                0,
                basis,
                ClockDomainId(1),
                logical(0),
                AudioAuthorityConfig {
                    history_capacity: 128,
                    max_observation_age: Duration::from_nanos(1_000_000_000),
                    input_extrapolation: ExtrapolationPolicy::Forbid,
                    max_input_ahead: Duration::ZERO,
                },
            )
            .unwrap();
        f.pause = NativePause::new(raw(0), ClockDomainId(1), 1000)
            .unwrap()
            .with_target_basis(0, basis)
            .unwrap();
        let mut target = TargetDevice {
            output,
            basis,
            step: 0,
            held: false,
            cancel: f.host.cancel.clone(),
            desired_pause: f.host.desired_pause.clone(),
            early_pause: 0,
            early_resume: 0,
            holds: vec![],
            target_frames: 2,
            presentation_delay_steps: 2,
            pause_flow: true,
            members: 2,
            delayed_inputs: None,
            rendered: vec![],
        };
        let result = run_cohort_audio_with_ports_and_speed(
            &mut target,
            AudioCohortSession {
                group: &mut f.group,
                network: Some(&mut f.network),
                states: &mut f.states,
                merger: &mut f.merger,
                bgm: &mut f.bgm,
                discipline: &mut f.presentation,
                pause: &mut f.pause,
                end: &mut f.end,
                delivery: &mut f.delivery,
                pre_origin_inputs: &mut f.pre,
            },
            f.config,
            &mut f.control,
            &mut f.host,
            f.setup,
        )
        .unwrap();
        assert!(result.is_none());
        assert!(
            target.early_pause > 0,
            "fixture must exercise source pause before target crossing"
        );
        assert!(
            target.early_resume > 0,
            "fixture must exercise source resume before target crossing"
        );
        assert_eq!(target.holds, [true, false, true, false]);
        assert_eq!(f.presentation.target_basis(), Some(basis));
        assert_eq!(
            f.host
                .pause_states
                .iter()
                .filter(|s| **s == PauseState::Paused)
                .count(),
            2
        );
        let resumed: Vec<_> = f
            .group
            .transport()
            .anchors()
            .windows(2)
            .filter(|pair| pair[0].rate == Rate::ZERO && pair[1].rate == f.setup.speed().rate())
            .collect();
        assert_eq!(resumed.len(), 2);
        for pair in resumed {
            assert_eq!(
                f.group
                    .transport()
                    .position_at(Timestamp::from_nanos(pair[1].host_time.as_nanos() + 1))
                    .unwrap(),
                Timestamp::from_nanos(pair[1].song_time.as_nanos() + delta)
            );
        }
        assert!(f.host.ends.is_empty());
        assert_eq!(f.network.completed, 0);
    }
}

fn assert_original_late_member_prefix(
    source: &beatkernel_bms::BmsChart,
    group: &RuntimeGroup,
    states: &[GameplayPlayerState<SoloObserver>],
    host_port: &Host,
) {
    assert_original_member_prefix(source, group, states, host_port, false);
}

fn assert_original_member_prefix(
    source: &beatkernel_bms::BmsChart,
    group: &RuntimeGroup,
    states: &[GameplayPlayerState<SoloObserver>],
    host_port: &Host,
    late_queue_race: bool,
) {
    assert!(group.gameplay_audio_sealed());
    assert_eq!(host_port.ends, [Timestamp::from_nanos(16_000_000)]);
    for (member, state) in states.iter().enumerate() {
        assert_eq!(state.score.hits, 1);
        assert_eq!(state.score.misses, 0);
        assert_eq!(state.last_song, Timestamp::from_nanos(16_000_000));
        assert_eq!(state.competition.as_ref().unwrap().completed, 1);
        let capture = state.capture.as_ref().unwrap();
        let inputs: Vec<_> = capture
            .records()
            .iter()
            .filter_map(|record| match &record.operation {
                ReplayOperation::Input(input) => Some((record, input)),
                _ => None,
            })
            .collect();
        assert_eq!(inputs.len(), 2);
        for (index, (original_ns, song_ns)) in [(9_000_000, 12_000_000), (9_500_000, 13_000_000)]
            .into_iter()
            .enumerate()
        {
            assert_eq!(inputs[index].0.song_time, Timestamp::from_nanos(song_ns));
            let meta = inputs[index].1.physical.meta();
            assert_eq!(meta.source, DeviceId(101 + member as u64));
            assert_eq!(meta.sequence, index as u64);
            assert_eq!(meta.original_clock_point, Some(host(original_ns)));
            assert_eq!(meta.timestamp, logical(original_ns).timestamp);
        }
        let reconstructed = reconstruct_section(
            source,
            beatkernel::replay::codec::ReplayFile::new(
                capture.header().clone(),
                capture.records().to_vec(),
            ),
            limits(),
        )
        .unwrap();
        assert_eq!(
            reconstructed.engine().stable_hash().unwrap(),
            group
                .member_judge(state.player)
                .unwrap()
                .stable_hash()
                .unwrap()
        );
        let actual_events: Vec<_> = host_port
            .reports
            .iter()
            .filter(|(id, _)| *id == state.player)
            .flat_map(|(_, report)| report.judge_events.iter().copied())
            .collect();
        assert_eq!(reconstructed.results(), actual_events);
        let mut rebuilt_gauge = BmsGauge::default();
        rebuilt_gauge.observe(reconstructed.results(), &[]).unwrap();
        assert_eq!(rebuilt_gauge, state.gauge);
        assert_eq!(
            capture.records().last().unwrap().song_time,
            Timestamp::from_nanos(16_000_000)
        );
        if !late_queue_race {
            assert!(
                host_port
                    .reports
                    .iter()
                    .filter(|(id, r)| *id == state.player && r.input.is_some())
                    .all(|(_, report)| report.audio_commands.is_empty()),
                "late original judge input cannot enqueue sound into closed output"
            );
        }
    }
    if late_queue_race {
        let sounds: Vec<_> = host_port
            .reports
            .iter()
            .filter(|(_, report)| !report.audio_commands.is_empty())
            .collect();
        assert_eq!(
            sounds.len(),
            1,
            "one genuine pre-seal input command reaches the late queue"
        );
        assert_eq!(sounds[0].1.audio_at, raw(10_000_000));
        assert!(sounds[0].1.audio_commands.iter().any(
            |command| matches!(command, AudioCommand::Play { at, .. } if *at == raw(10_000_000).timestamp)
        ));
    }
}

#[test]
fn unaligned_five_frame_source_callback_closes_pcm_but_preserves_late_original_cohort_inputs() {
    for members in [2, 4] {
        let mut f = Fixture::new(members, "2/1", 3_000_000, Some(16_000_000), None);
        f.device.block_frames = 5;
        f.device.delayed_inputs = Some((3, 9_000_000, 9_500_000));
        // The collector has not drained the 9ms events during the first two
        // blocks. Its explicit cut stops at 8ms, instead of inventing coverage.
        f.device.prefix_cap = Some((2, 8_000_000));
        let completed = f
            .run()
            .unwrap()
            .expect("actual source crossing must close every member");
        assert_eq!(completed.len(), members);
        assert_eq!(f.network.completed, 1);
        assert_original_late_member_prefix(&f.source, &f.group, &f.states, &f.host);
        let crossing = f
            .device
            .renders
            .iter()
            .find(|r| r.playback_end_physical_frame == Some(11))
            .unwrap();
        assert_eq!((crossing.start_frame, crossing.frames), (10, 5));
        assert_eq!(crossing.playback_frames, 1);
        assert!(crossing.paused);
        assert!(f
            .device
            .renders
            .iter()
            .all(|r| r.counters.commands_applied == 1
                && r.counters.commands_consumed == 1
                && r.pending_commands == 0));
        assert!(f.device.pcm.iter().all(|sample| *sample == 0.0));
        assert_eq!(f.merger.pending(), 0);
    }
}

#[test]
fn actual_two_and_four_member_stale_schedule_waits_for_late_published_command_expiry() {
    for members in [2, 4] {
        let mut f = Fixture::new(members, "2/1", 3_000_000, Some(16_000_000), None);
        f.device.block_frames = 5;
        f.device.delayed_inputs = Some((2, 9_000_000, 9_500_000));
        f.device.terminal_race = true;
        let completed = f
            .run()
            .unwrap()
            .expect("real silent callback settles terminal queue");
        assert_eq!(completed.len(), members);
        let prior = f.device.race_reports[0];
        let crossing = f.device.race_reports[1];
        assert_eq!(prior.playback_end_physical_frame, None);
        assert_eq!(
            prior.playback_start_frame + prior.playback_frames as u64,
            10
        );
        assert_eq!(crossing.playback_end_physical_frame, Some(11));
        assert_eq!(crossing.counters.commands_consumed, 1);
        assert_eq!(crossing.counters.commands_expired, 0);
        let sound_reports: Vec<_> = f
            .host
            .reports
            .iter()
            .filter(|(_, r)| !r.audio_commands.is_empty())
            .collect();
        assert_eq!(sound_reports.len(), 1);
        assert_eq!(sound_reports[0].1.audio_at, raw(10_000_000));
        assert!(sound_reports[0].1.audio_commands.iter().any(
            |c| matches!(c, AudioCommand::Play { at, .. } if *at == raw(10_000_000).timestamp)
        ));
        let terminal = f.device.report.unwrap();
        assert!(terminal.start_frame >= crossing.counters.rendered_frames);
        assert_eq!(terminal.counters.commands_consumed, 2);
        assert_eq!(terminal.counters.commands_applied, 1);
        assert_eq!(terminal.counters.commands_expired, 1);
        assert_eq!(terminal.pending_commands, 0);
        assert_eq!(terminal.song_position, crossing.song_position);
        assert_eq!(terminal.active_voices, crossing.active_voices);
        assert!(f.device.pcm.iter().all(|v| *v == 0.0));
        assert_eq!(f.network.completed, 1);
        assert_eq!(f.host.ends, [Timestamp::from_nanos(16_000_000)]);
        for (member, state) in f.states.iter().enumerate() {
            assert_eq!(state.score.hits, 1);
            assert_eq!(state.score.misses, 0);
            let capture = state.capture.as_ref().unwrap();
            let inputs: Vec<_> = capture
                .records()
                .iter()
                .filter_map(|r| match &r.operation {
                    ReplayOperation::Input(i) => Some((r, i)),
                    _ => None,
                })
                .collect();
            assert_eq!(inputs.len(), 2);
            assert_eq!(inputs[0].0.song_time, Timestamp::from_nanos(12_000_000));
            assert_eq!(
                inputs[0].1.physical.meta().original_clock_point,
                Some(host(9_000_000))
            );
            assert_eq!(
                inputs[0].1.physical.meta().source,
                DeviceId(101 + member as u64)
            );
            assert_eq!(inputs[0].1.physical.meta().sequence, 0);
            let replay = reconstruct_section(
                &f.source,
                beatkernel::replay::codec::ReplayFile::new(
                    capture.header().clone(),
                    capture.records().to_vec(),
                ),
                limits(),
            )
            .unwrap();
            assert_eq!(
                replay.engine().stable_hash().unwrap(),
                f.group
                    .member_judge(state.player)
                    .unwrap()
                    .stable_hash()
                    .unwrap()
            );
            let mut gauge = BmsGauge::default();
            gauge.observe(replay.results(), &[]).unwrap();
            assert_eq!(gauge, state.gauge);
        }
    }
}

#[test]
fn native_cohort_rejects_impossible_progression_after_owner_observes_real_terminal_marker() {
    for members in [2, 4] {
        for gaps in [false, true] {
            for mutation in TERMINAL_MUTATIONS {
                let mut f = Fixture::new(members, "2/1", 3_000_000, Some(16_000_000), None);
                f.device.block_frames = 5;
                f.device.delayed_inputs = Some((2, 9_000_000, 9_500_000));
                f.device.terminal_race = true;
                f.device.terminal_script = Some(TerminalScript::new(mutation, gaps));
                let result = f.run();
                f.device
                    .terminal_script
                    .as_ref()
                    .unwrap()
                    .assert_delivered();
                assert!(result.is_err(), "{members} members, {mutation:?}, intervening None/duplicate={gaps}: native owner accepted impossible terminal evidence");
                let actual = f.device.report.unwrap();
                assert_eq!(actual.counters.commands_consumed, 2);
                assert_eq!(actual.counters.commands_applied, 1);
                assert_eq!(actual.counters.commands_expired, 1);
                let sound_reports: Vec<_> = f
                    .host
                    .reports
                    .iter()
                    .filter(|(_, r)| !r.audio_commands.is_empty())
                    .collect();
                assert_eq!(sound_reports.len(), 1);
                assert_eq!(sound_reports[0].1.audio_at, raw(10_000_000));
                for (member, state) in f.states.iter().enumerate() {
                    let inputs: Vec<_> = state
                        .capture
                        .as_ref()
                        .unwrap()
                        .records()
                        .iter()
                        .filter_map(|r| match &r.operation {
                            ReplayOperation::Input(i) => Some(i),
                            _ => None,
                        })
                        .collect();
                    // Refusal can interrupt the acquired prefix. Every captured
                    // input must match an actually published owner report;
                    // completion, unlike this error path, drains both inputs.
                    let published: Vec<_> = f
                        .host
                        .reports
                        .iter()
                        .filter(|(id, report)| *id == state.player && report.input.is_some())
                        .map(|(_, report)| report.input.as_ref().unwrap())
                        .collect();
                    assert_eq!(inputs.len(), published.len());
                    assert!(inputs.len() <= 2);
                    for (index, (input, original)) in inputs.iter().zip(published).enumerate() {
                        let meta = input.physical.meta();
                        let original_ns = if index == 0 { 9_000_000 } else { 9_500_000 };
                        assert_eq!(meta, original.meta());
                        assert_eq!(meta.original_clock_point, Some(host(original_ns)));
                        assert_eq!(meta.source, DeviceId(101 + member as u64));
                        assert_eq!(meta.sequence, index as u64);
                        assert_eq!(meta.timestamp, logical(original_ns).timestamp);
                    }
                }
                assert!(f
                    .host
                    .reports
                    .iter()
                    .any(|(_, report)| report.input.is_some()));
                assert!(f.device.pcm.iter().all(|v| *v == 0.0));
            }
        }
    }
}

#[test]
fn native_cohort_accepts_real_terminal_expiry_duplicates_and_unavailable_intermediate_read() {
    for members in [2, 4] {
        let mut f = Fixture::new(members, "2/1", 3_000_000, Some(16_000_000), None);
        f.device.block_frames = 5;
        f.device.delayed_inputs = Some((2, 9_000_000, 9_500_000));
        f.device.terminal_race = true;
        f.device.terminal_script = Some(TerminalScript::new(TerminalMutation::Duplicate, true));
        assert_eq!(f.run().unwrap().unwrap().len(), members);
        f.device
            .terminal_script
            .as_ref()
            .unwrap()
            .assert_delivered();
        assert_eq!(
            f.device.report.unwrap().counters.settled_commands(),
            Some(2)
        );
        assert_original_member_prefix(&f.source, &f.group, &f.states, &f.host, true);
        assert!(f.device.pcm.iter().all(|v| *v == 0.0));
    }
}

#[test]
fn converted_target_marker_preserves_late_member_input_before_actual_target_end_crossing() {
    for members in [2, 4] {
        let mut f = Fixture::new(members, "2/1", 3_000_000, Some(16_000_000), None);
        let output = beatkernel_platform::audio::ConvertedNativeOutputState::new(
            f.device.mixer,
            beatkernel_platform::audio::DeviceFormat::new(
                2000,
                1,
                beatkernel_platform::audio::SampleEncoding::Float32,
                None,
            )
            .unwrap(),
            beatkernel::audio::ChannelMatrix::default_mix(1, 1).unwrap(),
            beatkernel::audio::ResampleQuality::Linear,
            16,
        )
        .unwrap_or_else(|_| panic!("valid independent finite converted output"));
        let basis = output.target_frame_basis();
        f.presentation =
            beatkernel_bms_runtime::native_audio_startup::new_target_audio_presentation(
                0,
                basis,
                ClockDomainId(1),
                logical(0),
                AudioAuthorityConfig {
                    history_capacity: 128,
                    max_observation_age: Duration::from_nanos(1_000_000_000),
                    input_extrapolation: ExtrapolationPolicy::Forbid,
                    max_input_ahead: Duration::ZERO,
                },
            )
            .unwrap();
        f.pause = NativePause::new(raw(0), ClockDomainId(1), 1000)
            .unwrap()
            .with_playback_end_frame(11)
            .unwrap()
            .with_target_basis(0, basis)
            .unwrap();
        f.end = Some(
            NativeEnd::new(raw(0), ClockDomainId(1), 1000, 11)
                .unwrap()
                .with_target_basis(0, basis)
                .unwrap(),
        );
        let mut target = TargetDevice {
            output,
            basis,
            step: 0,
            held: false,
            cancel: f.host.cancel.clone(),
            desired_pause: f.host.desired_pause.clone(),
            early_pause: 0,
            early_resume: 0,
            holds: vec![],
            target_frames: 10,
            presentation_delay_steps: 1,
            pause_flow: false,
            members,
            delayed_inputs: Some((3, 9_000_000, 9_500_000)),
            rendered: vec![],
        };
        let completed = run_cohort_audio_with_ports_and_speed(
            &mut target,
            AudioCohortSession {
                group: &mut f.group,
                network: Some(&mut f.network),
                states: &mut f.states,
                merger: &mut f.merger,
                bgm: &mut f.bgm,
                discipline: &mut f.presentation,
                pause: &mut f.pause,
                end: &mut f.end,
                delivery: &mut f.delivery,
                pre_origin_inputs: &mut f.pre,
            },
            f.config,
            &mut f.control,
            &mut f.host,
            f.setup,
        )
        .unwrap()
        .expect("genuine native target crossing");
        assert_eq!(completed.len(), members);
        assert_original_late_member_prefix(&f.source, &f.group, &f.states, &f.host);
        assert!(target
            .rendered
            .iter()
            .any(|r| r.playback_end_physical_frame == Some(11)));
        assert!(target
            .rendered
            .iter()
            .all(|r| r.counters.commands_consumed == 1
                && r.counters.commands_applied == 1
                && r.pending_commands == 0));
        let end = target.output.boundaries().end.unwrap();
        assert!(
            target.pair().source.timestamp
                >= end.target_time.point(basis.origin()).unwrap().timestamp,
            "source closure does not itself authorize converted target completion"
        );
        assert_eq!(f.network.completed, 1);
        assert_eq!(f.merger.pending(), 0);
    }
}
