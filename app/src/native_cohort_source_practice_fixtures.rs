//! Actual shared direct Source retained-practice entry. Original observations are DI,
//! not proof of acoustic timing or a selected hardware launcher.
use super::*;
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    bgm::BgmConfig,
    gameplay::output::ports::{
        NativePlaybackObservation, NativeSourcePlaybackQuery, SourcePlaybackFacts,
        SourcePlaybackIdentity,
    },
    gameplay_competition::{NoopGroupCompetition, NoopSoloCompetition},
    gameplay_presentation::GameplayDevice,
    local_runtime::MemberConfig,
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
    native_audio_startup::new_coordinate_audio_presentation,
    native_coordinate_control::CoordinateControlFacts,
    native_gameplay::InputBatch,
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
    native_playback_phase::NativePlaybackMapping,
    practice_control::{
        PracticeAction, PracticeCapability, PracticeReply, PracticeRequest,
        PracticeTransactionCapability, PracticeTransactionReply, PracticeTransactionRequest,
    },
    practice_playback::{
        PracticeMember, PracticePlayback, PracticeRecordingPort, PracticeRuntimeHeader,
    },
    practice_session::{PracticeAttemptConfig, prepare_attempt_with_speed},
    replay_speed::ReplaySpeed,
    session_launch::SessionLaunch,
};
use beatkernel::{
    audio::*,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId,
    },
    replay::{ReplaySession, codec::ReplayCodecLimits},
    runtime::{RuntimeProcessingClock, RuntimeReport},
    time::{ClockPair, Duration, ExtrapolationPolicy, presentation::PresentationEstimator},
    transport::{OutputPhaseBasis, Rate},
};
use beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn at(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn host(frame: u64) -> ClockPoint {
    at(1, 100 + i64::try_from(frame).unwrap() * 4)
}
fn exact(t: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(t, 0, 0, 1, 0, 1).unwrap()
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}

struct Device {
    mixer: Mixer,
    basis: OutputFrameBasis,
    report: RenderReport,
    step: Rc<Cell<usize>>,
    frame: u64,
    observation_stride: u64,
    last_acquired_frame: Option<u64>,
    close_at: usize,
    unavailable: bool,
    scalar_calls: usize,
    queries: RefCell<Vec<InterpolatedTargetTime>>,
    ids: Vec<u32>,
}
impl Device {
    fn observation(&self) -> CoordinateNativeAudioSnapshot {
        CoordinateNativeAudioSnapshot::new(
            NativeAudioSnapshot {
                epoch: 7,
                basis: self.basis,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                    source: self.basis.point_at_stream_frame(self.frame).unwrap(),
                    target: host(self.frame),
                }),
            },
            NativeOutputCoordinate::StreamFrame { frame: self.frame },
        )
        .unwrap()
    }
    fn generate(&mut self) -> NativeGameplayResult<()> {
        self.frame = 1 + self.step.get() as u64 * self.observation_stride;
        while self.mixer.frame_cursor() <= self.frame {
            self.report = self.mixer.render(&mut [0.0; 8])?;
        }
        Ok(())
    }
    fn packet(
        &self,
        q: InterpolatedTargetTime,
        id: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<NativeSourcePlaybackQuery<OriginalNativePresentationEvidence>> {
        let playback = self
            .mixer
            .source_playback_history()
            .unwrap()
            .lookup_at_source_time(q)
            .map_err(|error| format!("Source cohort practice lookup: {error:?}"))?;
        Ok(NativeSourcePlaybackQuery::new(
            7,
            self.basis,
            id,
            q,
            SourcePlaybackFacts {
                playback,
                pulled_source_end: self.mixer.frame_cursor(),
                submitted_source_end: self.mixer.frame_cursor(),
                render: Some(self.report),
                revision: 2 + 2 * self.frame,
            },
            Some(self.observation().snapshot().evidence),
        )?)
    }
}
impl GameplayDevice for Device {
    type Presentation = PresentationEstimator;
    fn observe_coordinate_playback_control(
        &mut self,
        p: &mut NativeAudioPresentation,
        id: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinatePlaybackControlFacts>> {
        self.generate()?;
        let obs = self.observation();
        p.admit_coordinate(obs)?;
        let q = exact(TargetTime::from_frames(self.frame, 1000)?);
        let native = NativePlaybackObservation::new(obs, self.packet(q, id)?)?;
        Ok(Some(CoordinatePlaybackControlFacts::Source(
            CoordinateControlFacts::from_source(&native, p, id)?,
        )))
    }
    fn playback_mapping_at(
        &self,
        _: &NativeAudioPresentation,
        q: &InterpolatedTargetTime,
        id: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<NativePlaybackMapping>> {
        self.queries.borrow_mut().push(*q);
        if self.unavailable {
            return Ok(None);
        }
        Ok(Some(NativePlaybackMapping::from_source(
            &self.packet(*q, id)?,
        )))
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(host(self.frame))
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(Some(self.report))
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        // A physical event is consumed once even when the pump continues without waiting.
        if self.step.get() > 10 && self.last_acquired_frame != Some(self.frame) {
            self.last_acquired_frame = Some(self.frame);
            for &id in &self.ids {
                events.push_back(PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(u64::from(id)), host(self.frame), self.frame),
                    control: PhysicalControlId::keyboard(4),
                    state: if self.frame % 2 == 0 {
                        ButtonState::Down
                    } else {
                        ButtonState::Up
                    },
                }));
            }
        }
        Ok(InputBatch {
            completed_through: Some(host(self.frame)),
            backlog: false,
            closed: self.step.get() >= self.close_at,
        })
    }
    fn observe(&mut self, _: &mut PresentationEstimator) -> NativeGameplayResult<()> {
        self.scalar_calls += 1;
        Err("scalar observation forbidden".into())
    }
    fn observe_audio(&mut self, p: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        self.generate()?;
        p.admit_coordinate(self.observation())?;
        Ok(())
    }
    fn observe_end(
        &mut self,
        _: &mut NativeEnd,
        _: &PresentationEstimator,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<crate::native_end::EndBoundary>> {
        self.scalar_calls += 1;
        Err("scalar endpoint observation forbidden".into())
    }
    fn seed_resume(
        &mut self,
        _: &mut PresentationEstimator,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        self.scalar_calls += 1;
        Err("coordinate practice cannot reseed scalar correlation".into())
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        Ok(at(2, 1_000_000_000))
    }
    fn publish_paused_audio_output(
        &mut self,
        _: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
        _: ClockPoint,
    ) -> NativeGameplayResult<bool> {
        Ok(false)
    }
}
struct Control(Rc<Cell<usize>>);
impl NativePumpControl for Control {
    type Moment = usize;
    fn now(&mut self) -> NativeGameplayResult<usize> {
        Ok(self.0.get())
    }
    fn checked_add(m: usize, _: WallDuration) -> Option<usize> {
        m.checked_add(1)
    }
    fn wait(&mut self, _: WallDuration) -> NativeGameplayResult<()> {
        let next = self.0.get() + 1;
        if next > 120 {
            return Err("practice fixture did not settle".into());
        }
        self.0.set(next);
        Ok(())
    }
}
struct Host {
    step: Rc<Cell<usize>>,
    scrub: bool,
    requested: bool,
    refuse: bool,
    generations: Vec<u64>,
    identities: Vec<u64>,
    commits: Vec<u64>,
    reports: Vec<PlayerReport>,
    replies: Vec<PracticeReply>,
    ends: Vec<Timestamp>,
    transaction_capabilities: Vec<PracticeTransactionCapability>,
    transaction_requests: VecDeque<PracticeTransactionRequest>,
    transaction_replies: Vec<PracticeTransactionReply>,
    transaction_taken: usize,
    transaction_at: usize,
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
    fn publish_section_end(&mut self, end: Timestamp) {
        self.ends.push(end);
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
    fn publish_report(&mut self, _: &RuntimeReport) -> NativeGameplayResult<()> {
        Err("solo report forbidden".into())
    }
    fn publish_local_reports(&mut self, r: &[PlayerReport]) -> NativeGameplayResult<()> {
        self.reports.extend_from_slice(r);
        Ok(())
    }
    fn advertise_practice(&mut self, cap: Option<PracticeCapability>) -> NativeGameplayResult<()> {
        if let Some(c) = cap {
            self.generations.push(c.generation);
        }
        Ok(())
    }
    fn take_practice_request(&mut self) -> NativeGameplayResult<Option<PracticeRequest>> {
        if self.scrub && !self.requested && !self.generations.is_empty() && self.step.get() >= 3 {
            self.requested = true;
            return Ok(Some(PracticeRequest {
                id: 12,
                generation: *self.generations.last().unwrap(),
                action: PracticeAction::Scrub {
                    target: Timestamp::from_nanos(2_000_000),
                },
            }));
        }
        Ok(None)
    }
    fn commit_practice_reply(&mut self, reply: &PracticeReply) -> NativeGameplayResult<()> {
        self.replies.push(reply.clone());
        Ok(())
    }
    fn advertise_practice_transactions(
        &mut self,
        cap: Option<PracticeTransactionCapability>,
    ) -> NativeGameplayResult<()> {
        if let Some(cap) = cap {
            self.transaction_capabilities.push(cap);
        }
        Ok(())
    }
    fn take_practice_transaction(
        &mut self,
        active: bool,
    ) -> NativeGameplayResult<Option<PracticeTransactionRequest>> {
        if active
            && !self.transaction_capabilities.is_empty()
            && self.step.get() >= self.transaction_at
            && self.transaction_taken == self.transaction_replies.len()
        {
            let next = self.transaction_requests.pop_front();
            self.transaction_taken += usize::from(next.is_some());
            Ok(next)
        } else {
            Ok(None)
        }
    }
    fn commit_practice_transaction_reply(
        &mut self,
        reply: &PracticeTransactionReply,
    ) -> NativeGameplayResult<()> {
        self.transaction_replies.push(reply.clone());
        Ok(())
    }
    fn prepare_practice_presentation(
        &mut self,
        generation: u64,
        attempts: &[(PlayerId, &crate::practice_session::PreparedPracticeAttempt)],
    ) -> NativeGameplayResult<crate::player::PreparedPracticePresentation> {
        crate::player::prepare_practice_presentation(generation, attempts)
    }
    fn apply_practice_identity(
        &mut self,
        p: &mut crate::player::PreparedPracticePresentation,
        g: u64,
    ) {
        crate::player::apply_practice_identity(p, g);
        self.identities.push(g);
    }
    fn commit_practice_presentation(
        &mut self,
        p: crate::player::PreparedPracticePresentation,
        g: u64,
    ) -> NativeGameplayResult<()> {
        if self.refuse {
            return Err("actual practice visual refusal".into());
        }
        crate::player::commit_practice_presentation(p, g)?;
        self.commits.push(g);
        Ok(())
    }
}
#[derive(Default)]
struct Recordings(Vec<(PlayerId, LiveReplayCapture)>);
impl PracticeRecordingPort for Recordings {
    fn archive(
        &mut self,
        id: PlayerId,
        _: &Path,
        c: LiveReplayCapture,
    ) -> NativeGameplayResult<()> {
        self.0.push((id, c));
        Ok(())
    }
}
struct Fixture {
    group: RuntimeGroup,
    states: Vec<GameplayPlayerState<NoopSoloCompetition>>,
    merger: InputMerger,
    p: NativeAudioPresentation,
    bgm: BgmFeeder,
    pause: NativePause,
    end: Option<NativeEnd>,
    delivery: InputDeliveryTelemetry,
    pre: u64,
    device: Device,
    control: Control,
    host: Host,
    practice: PracticePlayback,
    recordings: Recordings,
    config: CoordinateGameplayConfig,
    phase: OutputPhaseBasis,
    rate: Rate,
    original: beatkernel_bms::BmsChart,
}
impl Fixture {
    fn new(ids: &[u32], repeat: bool, scrub: bool) -> Self {
        Self::new_with_end(
            ids,
            repeat,
            scrub,
            Rate::new(3, 2).unwrap(),
            Timestamp::from_nanos(30_000_000),
        )
    }
    fn new_with_end(
        ids: &[u32],
        repeat: bool,
        scrub: bool,
        rate: Rate,
        end_song: Timestamp,
    ) -> Self {
        Self::new_mode(ids, repeat, scrub, rate, end_song, false, true)
    }
    fn new_runtime(ids: &[u32], record: bool) -> Self {
        Self::new_mode(
            ids,
            false,
            false,
            Rate::new(3, 2).unwrap(),
            Timestamp::from_nanos(100_000_000),
            true,
            record,
        )
    }
    fn new_mode(
        ids: &[u32],
        repeat: bool,
        scrub: bool,
        rate: Rate,
        end_song: Timestamp,
        runtime_rates: bool,
        record: bool,
    ) -> Self {
        let speed = ReplaySpeed::new(rate).unwrap();
        let setup =
            LiveSpeedSetup::new(Timestamp::ZERO, Duration::ZERO, speed, Some(end_song)).unwrap();
        let original = beatkernel_bms::parse(
            "#BPM 120\n#WAV01 key.wav\n#00011:01\n#00111:01\n",
            Default::default(),
        )
        .unwrap();
        let policy =
            crate::play_policy::ResolvedPlayPolicy::builtin(10_000_000, 10_000_000, 0).unwrap();
        let format = AudioFormat::new(1000, 1).unwrap();
        let bank = SampleBank::new(format, PcmLimits::new(8, 256, 1).unwrap()).unwrap();
        let program_limits = PracticeLimits::new(8, 1, 64, 8, 256).unwrap();
        let program = if runtime_rates {
            PreparedPracticeProgram::new_with_runtime_rates(
                &bank,
                vec![],
                program_limits,
                rate,
                Duration::ZERO,
                false,
                14102,
            )
        } else {
            PreparedPracticeProgram::new_with_speed(
                &bank,
                vec![],
                program_limits,
                rate,
                Duration::ZERO,
                false,
            )
        }
        .unwrap();
        let (controller, endpoint) = practice_queue(&program).unwrap();
        let (producer, consumer) = command_queue(256).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(2),
                Timestamp::ZERO,
                AudioLimits::new(256, 4, 8, 256, 8).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        let region = PracticeRegion::new(Timestamp::ZERO, end_song, repeat).unwrap();
        mixer.install_practice(program, endpoint, region).unwrap();
        mixer.enable_source_playback_history(128).unwrap();
        let source_basis = mixer.output_frame_basis();
        let basis = source_basis;
        let initial_report = if runtime_rates {
            None
        } else {
            Some(mixer.render(&mut [0.0; 8]).unwrap())
        };
        let phase = OutputPhaseBasis::new(ClockDomainId(3), 91, 1, 0, 1000).unwrap();
        let mut members = Vec::new();
        let mut runtime_headers = Vec::new();
        let mut configs = Vec::new();
        let mut states = Vec::new();
        let mut transport = None;
        for &id in ids {
            let mut args = vec![
                "--chart".into(),
                "practice.bms".into(),
                "--speed".into(),
                format!("{}/{}", rate.numerator(), rate.denominator()),
            ];
            if record {
                args.extend(["--record-replay".into(), format!("player-{id}.bkr")]);
            }
            let launch = SessionLaunch::new(args).unwrap();
            let attempt = prepare_attempt_with_speed(
                &original,
                &policy,
                &launch,
                PracticeAttemptConfig {
                    start: Timestamp::ZERO,
                    end: Some(end_song),
                    domain: ClockDomainId(3),
                    chart_seed: 0,
                    capture_limits: record.then_some(limits()),
                },
                speed,
            )
            .unwrap();
            transport = Some(if runtime_rates {
                attempt
                    .transport_at_runtime_output_phase(phase, at(3, 0))
                    .unwrap()
            } else {
                attempt.transport_at_output_phase(phase, at(3, 0)).unwrap()
            });
            if runtime_rates {
                let header = if let Some(capture) = &attempt.capture {
                    capture.header().clone()
                } else {
                    LiveReplayCapture::new_with_policy_and_speed(
                        &attempt.judge,
                        ClockDomainId(3),
                        limits(),
                        Timestamp::ZERO,
                        0,
                        Some(end_song),
                        beatkernel_bms::BmsInputMode::ButtonOnly,
                        None,
                        &policy,
                        speed,
                    )
                    .unwrap()
                    .header()
                    .clone()
                };
                runtime_headers.push(PracticeRuntimeHeader {
                    player: PlayerId(id),
                    header,
                    limits: limits(),
                });
            }
            configs.push(MemberConfig {
                player: PlayerId(id),
                device: Some(DeviceId(u64::from(id))),
                bindings: BindingMap::from_bindings([Binding {
                    device: DeviceSelector::Exact(DeviceId(u64::from(id))),
                    physical: PhysicalControlId::keyboard(4),
                    game_control: GameControlId(0x11),
                }])
                .unwrap(),
                judge: attempt.judge,
                sounds: vec![],
            });
            states.push(GameplayPlayerState {
                player: PlayerId(id),
                capture: attempt.capture,
                competition: None,
                completion: None,
                score: attempt.score,
                gauge: attempt.gauge,
                last_song: Timestamp::ZERO,
            });
            members.push(PracticeMember {
                player: PlayerId(id),
                policy: crate::play_policy::ResolvedPlayPolicy::builtin(10_000_000, 10_000_000, 0)
                    .unwrap(),
                launch,
                chart_seed: 0,
                capture_limits: record.then_some(limits()),
            });
        }
        let practice = if runtime_rates {
            PracticePlayback::new_with_runtime_rates(
                controller,
                original.clone(),
                members,
                region,
                Timestamp::from_nanos(100_000_000),
                source_basis,
                Some(end_song),
                setup,
                runtime_headers,
            )
        } else {
            PracticePlayback::new_with_speed(
                controller,
                original.clone(),
                members,
                region,
                Timestamp::from_nanos(100_000_000),
                source_basis,
                2,
                Some(end_song),
                setup,
            )
        }
        .unwrap();
        let report = initial_report.unwrap_or_else(|| mixer.render(&mut [0.0; 8]).unwrap());
        let mut group = RuntimeGroup::new(
            ClockDomainId(3),
            ClockDomainId(2),
            transport.unwrap(),
            producer,
            configs,
            32,
            &[],
        )
        .unwrap();
        group.set_processing_clock(RuntimeProcessingClock::Disabled);
        group.set_audio_scope(CommandScope(1));
        group.set_song_end(end_song).unwrap();
        let step = Rc::new(Cell::new(0));
        let device = Device {
            mixer,
            basis,
            report,
            step: step.clone(),
            frame: 1,
            observation_stride: 1,
            last_acquired_frame: None,
            close_at: 55,
            unavailable: false,
            scalar_calls: 0,
            queries: RefCell::new(vec![]),
            ids: ids.to_vec(),
        };
        let mut p = new_coordinate_audio_presentation(
            7,
            AudioAuthorityCoordinateBasis::SourceFrame(basis),
            ClockDomainId(1),
            at(3, 0),
            AudioAuthorityConfig {
                history_capacity: 128,
                max_observation_age: Duration::from_nanos(100),
                input_extrapolation: ExtrapolationPolicy::Bounded {
                    before: Duration::ZERO,
                    after: Duration::ZERO,
                },
                max_input_ahead: Duration::ZERO,
            },
        )
        .unwrap();
        let cold = CoordinateNativeAudioSnapshot::new(
            NativeAudioSnapshot {
                epoch: 7,
                basis,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                    source: basis.point_at_stream_frame(0).unwrap(),
                    target: host(0),
                }),
            },
            NativeOutputCoordinate::StreamFrame { frame: 0 },
        )
        .unwrap();
        p.admit_coordinate(cold).unwrap();
        p.admit_coordinate(device.observation()).unwrap();
        p.pin_practice_mixer_basis(source_basis).unwrap();
        let stream = p.authority().epoch().stream_origin;
        Self {
            group,
            states,
            merger: InputMerger::new(
                ClockDomainId(1),
                host(0),
                ids.iter().map(|&id| DeviceId(u64::from(id))).collect(),
                256,
            )
            .unwrap(),
            p,
            bgm: BgmFeeder::new(
                vec![],
                BgmConfig {
                    output_origin: source_basis.origin(),
                    sample_rate: 1000,
                    preroll: Duration::ZERO,
                    lookahead: Duration::from_nanos(10_000_000),
                    max_pending: 8,
                },
            )
            .unwrap(),
            pause: NativePause::new(source_basis.origin(), ClockDomainId(1), 1000)
                .unwrap()
                .with_coordinate_binding(
                    7,
                    AudioAuthorityCoordinateBasis::SourceFrame(source_basis),
                    SourcePlaybackIdentity::new(1000, 0, 0).unwrap(),
                )
                .unwrap(),
            end: None,
            delivery: InputDeliveryTelemetry::new(128, ClockDomainId(1)).unwrap(),
            pre: 0,
            device,
            control: Control(step.clone()),
            host: Host {
                step,
                scrub,
                requested: false,
                refuse: false,
                generations: vec![],
                identities: vec![],
                commits: vec![],
                reports: vec![],
                replies: vec![],
                ends: vec![],
                transaction_capabilities: vec![],
                transaction_requests: VecDeque::new(),
                transaction_replies: vec![],
                transaction_taken: 0,
                transaction_at: 14,
            },
            practice,
            recordings: Recordings::default(),
            config: CoordinateGameplayConfig {
                audio: AudioGameplayConfig {
                    gameplay: NativeGameplayConfig {
                        origin: host(0),
                        stream_origin: stream,
                        playback_origin: stream,
                        song_origin: Timestamp::ZERO,
                        sample_rate: 1000,
                        end_song: Some(end_song),
                        advance_lag: Duration::ZERO,
                        seconds: None,
                        pause_supported: false,
                        logical_schedule: false,
                    },
                    section_start: Timestamp::ZERO,
                },
                source_history: SourcePlaybackIdentity::new(1000, 0, 0).unwrap(),
            },
            phase,
            rate,
            original,
        }
    }
    fn run(&mut self) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
        let session: AudioCohortSession<'_, NoopSoloCompetition, NoopGroupCompetition> =
            CohortSession {
                group: &mut self.group,
                network: None,
                states: &mut self.states,
                merger: &mut self.merger,
                bgm: &mut self.bgm,
                discipline: &mut self.p,
                pause: &mut self.pause,
                end: &mut self.end,
                delivery: &mut self.delivery,
                pre_origin_inputs: &mut self.pre,
            };
        run_cohort_audio_coordinate_practice_with_results_and_ports(
            &mut self.device,
            session,
            self.config,
            &mut self.control,
            &mut self.host,
            &mut self.practice,
            &mut self.recordings,
        )
    }
    fn assert_fresh(&self) {
        let basis = self.group.transport().output_phase_basis().unwrap();
        assert_eq!(basis.owner(), self.phase.owner());
        assert_eq!(basis.domain(), self.phase.domain());
        assert_eq!(basis.source_hz(), 1000);
        assert_eq!(basis.generation(), self.practice.generation());
        assert!(basis.generation() > 1);
        assert!(basis.active_origin_frame() > 0);
        assert_eq!(self.device.scalar_calls, 0);
        assert!(!self.group.poisoned());
        for state in &self.states {
            let capture = state.capture.as_ref().unwrap();
            let setup =
                crate::replay_playback::decode_section_setup(&capture.header().options).unwrap();
            assert_eq!(setup.source_speed.rate(), self.rate);
            let selected = crate::section_start::source_at(&self.original, setup.start).unwrap();
            let policy =
                crate::play_policy::ResolvedPlayPolicy::builtin(10_000_000, 10_000_000, 0).unwrap();
            let judge = beatkernel::judge::JudgeEngine::new(
                selected.compile().unwrap().chart,
                selected.rules(),
                policy.judge().clone(),
            )
            .unwrap();
            let replay = ReplaySession::from_records(
                capture.header().clone(),
                judge,
                capture.records().to_vec(),
            )
            .unwrap();
            assert_eq!(
                replay.engine().stable_hash().unwrap(),
                self.group
                    .member_judge(state.player)
                    .unwrap()
                    .stable_hash()
                    .unwrap()
            );
            assert!(capture
                .records()
                .iter()
                .any(|r| matches!(r.operation, beatkernel::replay::ReplayOperation::Input(_))));
        }
        assert_eq!(self.p.authority().epoch().id, 7);
        assert_eq!(
            self.p.authority().coordinate_basis(),
            Some(AudioAuthorityCoordinateBasis::SourceFrame(
                self.device.basis
            ))
        );
    }
}

#[test]
fn direct_source_equal_cut_is_fresh_for_normal_and_selected_two_and_four_members() {
    for rate in [Rate::NORMAL, Rate::new(3, 2).unwrap()] {
        for ids in [&[7, 29][..], &[7, 29, 101, 409][..]] {
            let mut f =
                Fixture::new_with_end(ids, true, false, rate, Timestamp::from_nanos(31_000_000));
            let cut = if rate == Rate::NORMAL { 31 } else { 21 };
            f.device.close_at = cut as usize + 10;
            assert!(f.run().unwrap().is_none());
            f.assert_fresh();
            let basis = f.group.transport().output_phase_basis().unwrap();
            assert_eq!(basis.generation(), 2);
            assert_eq!(basis.active_origin_frame(), cut);
            assert_eq!(f.group.audio_scope(), CommandScope(2));
            assert_eq!(f.recordings.0.len(), ids.len());
            let q = exact(TargetTime::from_frames(cut, 1000).unwrap());
            assert!(f
                .device
                .queries
                .borrow()
                .iter()
                .any(|query| query.compare_interpolated_time(q).is_eq()));
            let packet = f.device.packet(q, f.config.source_history).unwrap();
            let history = f.device.mixer.source_playback_history().unwrap();
            assert_eq!(history.source_rate(), 1000);
            assert_eq!(history.initial_physical_frame(), 0);
            assert_eq!(history.initial_playback_frame(), 0);
            let span = packet.facts().playback;
            assert_eq!(span.physical_start(), 0);
            assert_eq!(span.playback_start(), 0);
            assert_eq!(span.state(), SourcePlaybackState::Running);
            assert_eq!(span.elapsed_nanos_at_source_time(q, 1000, cut).unwrap(), 0);
            for (player, old) in &f.recordings.0 {
                assert!(ids.contains(&player.0));
                assert!(!old
                    .records()
                    .iter()
                    .any(|record| matches!(&record.operation,
                    beatkernel::replay::ReplayOperation::Input(input)
                    if input.physical.meta().sequence == cut)));
            }
            for state in &f.states {
                let inputs = state
                    .capture
                    .as_ref()
                    .unwrap()
                    .records()
                    .iter()
                    .filter_map(|record| match &record.operation {
                        beatkernel::replay::ReplayOperation::Input(input)
                            if input.physical.meta().sequence == cut
                                && input.physical.meta().source
                                    == DeviceId(u64::from(state.player.0)) =>
                        {
                            Some((record.song_time, input))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(inputs.len(), 1, "one original event at the new shared cut");
                assert_eq!(inputs[0].0, Timestamp::ZERO);
                assert_eq!(
                    inputs[0].1.physical.meta().original_clock_point,
                    Some(host(cut))
                );
            }
        }
    }
}

#[test]
fn actual_source_cohort_loop_preserves_every_fresh_attempt_under_a_lagged_prefix() {
    for stop_at in [7, 8] {
        let ids = [7, 29];
        let mut f = Fixture::new_with_end(
            &ids,
            true,
            false,
            Rate::NORMAL,
            Timestamp::from_nanos(30_000_000),
        );
        let pristine: Vec<_> = f
            .states
            .iter()
            .map(|state| {
                (
                    f.group
                        .member_judge(state.player)
                        .unwrap()
                        .stable_hash()
                        .unwrap(),
                    *state.gauge.snapshot(),
                )
            })
            .collect();
        f.device.ids.clear();
        f.device.observation_stride = 5;
        f.device.close_at = stop_at;
        f.config.audio.gameplay.advance_lag = Duration::from_nanos(2);
        // Original HOST=100+4*frame ns. A26<C30<B31 and prefixHOST222
        // (=Source30.5ms) cover the cut while selecting original A26.
        assert!(f.run().unwrap().is_none());
        let fresh_basis = f.group.transport().output_phase_basis().unwrap();
        assert_eq!(fresh_basis.active_origin_frame(), 30);
        assert_eq!(fresh_basis.generation(), 2);
        assert_eq!(f.group.audio_scope(), CommandScope(2));
        assert_eq!(f.host.identities, [2]);
        assert_eq!(f.host.commits, [2]);
        assert!(!f.practice.transition_pending());
        assert!(f.practice.peek_receipt().unwrap().is_none());
        assert!(f.device.queries.borrow().iter().any(|query| query
            .compare_target_time(TargetTime::from_frames(26, 1000).unwrap())
            .is_eq()));
        assert_eq!(f.recordings.0.len(), ids.len());
        if stop_at == 7 {
            for (index, state) in f.states.iter().enumerate() {
                assert_eq!(state.last_song, Timestamp::ZERO);
                assert_eq!(
                    f.group
                        .member_judge(state.player)
                        .unwrap()
                        .stable_hash()
                        .unwrap(),
                    pristine[index].0
                );
                assert_eq!(
                    f.group
                        .member_judge(state.player)
                        .unwrap()
                        .effective_song_time(),
                    None
                );
                assert_eq!(*state.gauge.snapshot(), pristine[index].1);
                assert_eq!((state.score.hits, state.score.misses), (0, 0));
                assert!(state.capture.as_ref().unwrap().records().is_empty());
            }
        }
        for (player, old) in &f.recordings.0 {
            assert!(ids.contains(&player.0));
            assert_eq!(
                old.records().last().unwrap().song_time,
                Timestamp::from_nanos(30_000_000)
            );
            assert!(old.records().iter().all(|record| matches!(
                record.operation,
                beatkernel::replay::ReplayOperation::Advance
            )));
        }

        // The next acquired prefix covers the genuine B31 observation already
        // retained by the owner, so each new attempt advances31-30 =1ms.
        if stop_at == 7 {
            continue;
        }
        assert_eq!(f.group.transport().output_phase_basis(), Some(fresh_basis));
        assert_eq!(f.practice.generation(), 2);
        assert_eq!(f.host.identities, [2]);
        assert_eq!(f.host.commits, [2]);
        assert_eq!(f.recordings.0.len(), ids.len());
        assert_eq!(f.device.scalar_calls, 0);
        assert!(!f.group.poisoned());
        for state in &f.states {
            assert_eq!(state.last_song, Timestamp::from_nanos(1_000_000));
            assert_eq!(
                f.group
                    .member_judge(state.player)
                    .unwrap()
                    .effective_song_time(),
                Some(Timestamp::from_nanos(1_000_000))
            );
            let capture = state.capture.as_ref().unwrap();
            let setup =
                crate::replay_playback::decode_section_setup(&capture.header().options).unwrap();
            assert_eq!(setup.start, Timestamp::ZERO);
            assert_eq!(setup.source_speed.rate(), Rate::NORMAL);
            assert_eq!(
                capture.records().last().unwrap().song_time,
                Timestamp::from_nanos(1_000_000)
            );
            let replay = crate::replay_playback::reconstruct_section(
                &f.original,
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
        }
    }
}

#[test]
fn genuine_source_scrub_preserves_selected_phase_and_canonical_shared_replays() {
    for rate in [Rate::NORMAL, Rate::new(3, 2).unwrap()] {
        for ids in [&[7, 29][..], &[7, 29, 101, 409][..]] {
            let mut f =
                Fixture::new_with_end(ids, false, true, rate, Timestamp::from_nanos(60_000_000));
            f.device.close_at = 25;
            assert!(f.run().unwrap().is_none());
            f.assert_fresh();
            assert!(f
                .host
                .replies
                .iter()
                .any(|r| r.id == 12 && r.result.is_ok()));
            assert_eq!(f.recordings.0.len(), ids.len());
            assert!(f.host.commits.contains(&2));
        }
    }
}

#[test]
fn repeated_source_loops_archive_sparse_cohort_on_one_original_clock() {
    for ids in [&[7, 29][..], &[7, 29, 101, 409][..]] {
        let mut f = Fixture::new(ids, true, false);
        assert!(f.run().unwrap().is_none());
        f.assert_fresh();
        assert!(f.practice.generation() >= 3);
        assert!(f.recordings.0.len() >= 2 * ids.len());
        assert_eq!(f.recordings.0.len() % ids.len(), 0);
        for archive in f.recordings.0.chunks(ids.len()) {
            assert_eq!(archive.iter().map(|(id, _)| id.0).collect::<Vec<_>>(), ids);
            let ends = archive
                .iter()
                .map(|(_, c)| c.records().last().unwrap().song_time)
                .collect::<Vec<_>>();
            assert!(ends.iter().all(|end| *end == ends[0]));
        }
    }
}

#[test]
fn missing_source_mapping_waits_without_whole_cohort_archive_or_identity_change() {
    let mut f = Fixture::new(&[7, 29, 101, 409], true, false);
    f.device.unavailable = true;
    f.device.close_at = 35;
    assert!(f.run().unwrap().is_none());
    assert!(f.recordings.0.is_empty());
    assert_eq!(f.group.transport().output_phase_basis(), Some(f.phase));
    assert_eq!(f.group.audio_scope(), CommandScope(1));
    assert!(f.host.commits.is_empty());
    assert!(f.host.identities.is_empty());
    assert_eq!(f.practice.generation(), 1);
    assert!(!f.device.queries.borrow().is_empty());
    assert_eq!(f.device.scalar_calls, 0);
    assert!(!f.group.poisoned());
}

#[test]
fn source_visual_refusal_preserves_applied_whole_cohort_phase_identity() {
    let mut f = Fixture::new(&[7, 29, 101, 409], true, false);
    f.host.refuse = true;
    assert!(f.run().is_err());
    assert_eq!(f.recordings.0.len(), 4);
    assert_eq!(f.host.identities, [2]);
    assert!(f.host.commits.is_empty());
    let basis = f.group.transport().output_phase_basis().unwrap();
    assert_eq!(basis.generation(), 2);
    assert_eq!(basis.owner(), f.phase.owner());
    assert_eq!(basis.source_hz(), 1000);
    assert_eq!(f.group.audio_scope(), CommandScope(2));
    assert!(f
        .states
        .iter()
        .all(|state| state.capture.as_ref().unwrap().records().is_empty()));
    assert_eq!(f.p.authority().epoch().id, 7);
    assert_eq!(
        f.p.authority().coordinate_basis(),
        Some(AudioAuthorityCoordinateBasis::SourceFrame(f.device.basis))
    );
    assert!(!f.group.poisoned());
}

fn request_runtime_rate(f: &mut Fixture, selected: Rate) {
    f.host
        .transaction_requests
        .push_back(PracticeTransactionRequest::Rate {
            id: 14102,
            generation: f.practice.generation(),
            expected: f.practice.rate_state().unwrap(),
            requested_rate: selected,
        });
}
fn applied_runtime_rate(f: &Fixture) -> PracticeRateEvent {
    match &f.host.transaction_replies[..] {
        [
            PracticeTransactionReply::Rate {
                id: 14102,
                generation: 1,
                result: Ok(event),
            },
        ] => *event,
        replies => panic!("one actual cohort rate reply required: {replies:?}"),
    }
}
#[test]
fn runtime_source_cohort_shares_one_native_cut_and_effective_headers_with_or_without_capture() {
    for ids in [&[7, 29][..], &[7, 29, 101, 409][..]] {
        for record in [true, false] {
            let mut f = Fixture::new_runtime(ids, record);
            f.device.close_at = 27;
            let initial = f.practice.current_speed();
            let selected = Rate::new(1, 2).unwrap();
            request_runtime_rate(&mut f, selected);
            assert!(f.run().unwrap().is_none());
            let event = applied_runtime_rate(&f);
            assert_eq!(event.outcome, PracticeRateOutcome::Applied);
            assert_eq!(event.previous_rate, initial.rate());
            assert_eq!(event.applied_rate, selected);
            assert_eq!(f.practice.generation(), 1);
            assert_eq!(f.group.transport().output_phase_basis(), Some(f.phase));
            assert_eq!(f.group.transport().anchor().rate, selected);
            assert_eq!(f.group.audio_scope(), CommandScope(1));
            assert!(
                f.host.identities.is_empty(),
                "no member score/held key reset"
            );
            assert!(f.host.commits.is_empty());
            assert!(f.recordings.0.is_empty());
            assert_eq!(f.device.scalar_calls, 0);
            assert!(!f.group.poisoned());
            let mut common_cut = None;
            for state in &f.states {
                let header = f.practice.effective_header(state.player).unwrap();
                let (_, schedule) =
                    crate::replay_speed_policy::split_schedule(&header.options).unwrap();
                assert_eq!(schedule.initial_speed(), initial);
                assert_eq!(schedule.current_speed().rate(), selected);
                assert_eq!(schedule.cuts().len(), 1);
                let cut = schedule.cuts()[0];
                if let Some(previous) = common_cut {
                    assert_eq!(cut, previous);
                } else {
                    common_cut = Some(cut);
                }
                assert!(
                    state.score.hits + state.score.misses > 0,
                    "existing judged stages survive the rate cut"
                );
                if record {
                    let capture = state.capture.as_ref().unwrap();
                    assert_eq!(capture.header(), header);
                    let inputs: Vec<_> = capture
                        .records()
                        .iter()
                        .filter_map(|r| match &r.operation {
                            beatkernel::replay::ReplayOperation::Input(input) => Some(input),
                            _ => None,
                        })
                        .collect();
                    assert!(!inputs.is_empty());
                    assert!(
                        inputs.iter().all(|input| input.physical.meta().source
                            == DeviceId(u64::from(state.player.0)))
                    );
                    let replay = crate::replay_playback::reconstruct_section(
                        &f.original,
                        beatkernel::replay::codec::ReplayFile::new(
                            header.clone(),
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
                } else {
                    assert!(state.capture.is_none());
                }
            }
        }
    }
}
#[test]
fn runtime_source_cohort_noop_and_stale_revision_preserve_every_member_header() {
    for stale in [false, true] {
        let mut f = Fixture::new_runtime(&[7, 29, 101, 409], true);
        f.device.close_at = 27;
        let initial = f.practice.current_speed();
        let before: Vec<_> = f
            .states
            .iter()
            .map(|s| (s.player, s.capture.as_ref().unwrap().header().clone()))
            .collect();
        request_runtime_rate(
            &mut f,
            if stale {
                Rate::new(1, 2).unwrap()
            } else {
                initial.rate()
            },
        );
        if stale {
            if let Some(PracticeTransactionRequest::Rate { expected, .. }) =
                f.host.transaction_requests.front_mut()
            {
                expected.revision = 99;
            }
        }
        f.run().unwrap();
        assert_eq!(f.practice.current_speed(), initial);
        assert_eq!(f.practice.generation(), 1);
        assert_eq!(f.group.transport().output_phase_basis(), Some(f.phase));
        assert!(f.recordings.0.is_empty());
        assert!(f.host.identities.is_empty());
        assert_eq!(f.host.transaction_replies.len(), 1);
        match &f.host.transaction_replies[0] {
            PracticeTransactionReply::Rate { result: Err(_), .. } if stale => {}
            PracticeTransactionReply::Rate {
                result: Ok(event), ..
            } if !stale => assert_eq!(event.outcome, PracticeRateOutcome::Noop),
            reply => panic!("unexpected shared noop/refusal: {reply:?}"),
        }
        for (id, header) in before {
            assert_eq!(f.practice.effective_header(id), Some(&header));
            assert_eq!(
                f.states
                    .iter()
                    .find(|s| s.player == id)
                    .unwrap()
                    .capture
                    .as_ref()
                    .unwrap()
                    .header(),
                &header
            );
        }
    }
}

#[test]
fn runtime_source_cohort_f5_compound_restart_archives_rate_lineage_and_restores_original_speed_once()
 {
    let mut f = Fixture::new_runtime(&[7, 29], true);
    f.device.close_at = 27;
    let initial = f.practice.current_speed().rate();
    let changed = Rate::new(1, 2).unwrap();
    request_runtime_rate(&mut f, changed);
    f.host
        .transaction_requests
        .push_back(PracticeTransactionRequest::RestartOriginal {
            id: 14103,
            generation: 1,
            expected: crate::practice_control::PracticeRateState {
                revision: 1,
                rate: changed,
            },
        });
    f.run().unwrap();
    assert_eq!(f.host.transaction_replies.len(), 2);
    assert!(matches!(
        &f.host.transaction_replies[0],
        PracticeTransactionReply::Rate {
            result: Ok(PracticeRateEvent {
                outcome: PracticeRateOutcome::Applied,
                ..
            }),
            ..
        }
    ));
    assert!(matches!(
        &f.host.transaction_replies[1],
        PracticeTransactionReply::RestartOriginal {
            id: 14103,
            result: Ok(_),
            ..
        }
    ));
    assert_eq!(f.practice.generation(), 2);
    assert_eq!(f.practice.current_speed().rate(), initial);
    assert_eq!(f.group.transport().anchor().rate, initial);
    assert_eq!(f.host.identities, [2]);
    assert_eq!(f.host.commits, [2]);
    assert_eq!(f.recordings.0.len(), 2);
    for (player, archived) in &f.recordings.0 {
        let (_, old_schedule) =
            crate::replay_speed_policy::split_schedule(&archived.header().options).unwrap();
        assert_eq!(old_schedule.cuts().len(), 1);
        assert_eq!(old_schedule.current_speed().rate(), changed);
        let current = f.practice.effective_header(*player).unwrap();
        let (_, fresh_schedule) =
            crate::replay_speed_policy::split_schedule(&current.options).unwrap();
        assert!(fresh_schedule.cuts().is_empty());
        assert_eq!(fresh_schedule.current_speed().rate(), initial);
    }
}
