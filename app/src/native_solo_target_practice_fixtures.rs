//! Actual retained solo pump and converted receipts; DI pairs are not hardware timing proof.
use super::*;
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    bgm::BgmConfig,
    gameplay::output::ports::{
        NativeTargetPlaybackFacts, NativeTargetPlaybackObservation, NativeTargetPlaybackQuery,
        SourcePlaybackIdentity,
    },
    gameplay_competition::NoopSoloCompetition,
    gameplay_presentation::GameplayDevice,
    native_audio_presentation::TargetNativeAudioSnapshot,
    native_audio_startup::new_coordinate_audio_presentation,
    native_coordinate_control::{CoordinatePlaybackControlFacts, TargetCoordinateControlFacts},
    native_output_coordinate::{CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate},
    native_playback_phase::{ConvertedPlaybackFacts, NativePlaybackMapping},
    play_policy::ResolvedPlayPolicy,
    practice_control::{
        PracticeAction, PracticeCapability, PracticeReply, PracticeRequest,
        PracticeTransactionCapability, PracticeTransactionReply, PracticeTransactionRequest,
    },
    practice_playback::{
        PracticeMember, PracticePlayback, PracticeRecordingPort, PracticeRuntimeHeader,
    },
    replay_speed::ReplaySpeed,
    session_launch::SessionLaunch,
};
use beatkernel::{
    audio::*,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId,
    },
    judge::JudgeEngine,
    replay::{ReplayOperation, codec::ReplayCodecLimits},
    runtime::RuntimeProcessingClock,
    time::presentation::PresentationEstimator,
    transport::{OutputPhaseBasis, Rate, Transport},
};
use beatkernel_platform::audio::{
    ConvertedNativeOutputState, DeviceFormat, SampleEncoding,
    presentation::validation::OriginalNativePresentationEvidence,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn snapshot(basis: TargetFrameBasis, frame: u64) -> CoordinateTargetNativeAudioSnapshot {
    CoordinateTargetNativeAudioSnapshot::new(
        TargetNativeAudioSnapshot {
            epoch: 0,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(frame).unwrap(),
                target: point(1, 100 + (u128::from(frame) * 1_000_000_000 / 1500) as i64),
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap()
}

struct Device {
    owner: ConvertedNativeOutputState,
    report: ConvertedRenderReport,
    basis: TargetFrameBasis,
    lap: Rc<Cell<usize>>,
    stop_at: usize,
    mapping_missing: bool,
    events: VecDeque<PhysicalInputEvent>,
    pins: Vec<(u64, TargetTime)>,
    first_block: usize,
    block_schedule: &'static [usize],
    unprojected_at: Option<(usize, u64, u64)>,
    acquired: Vec<(usize, ClockPoint, EventMeta)>,
    mapped: RefCell<Vec<(InterpolatedTargetTime, ConvertedPlaybackFacts)>>,
}
impl Device {
    fn now(&self) -> ClockPoint {
        match snapshot(self.basis, self.report.target_frame_cursor)
            .snapshot()
            .evidence
        {
            OriginalNativePresentationEvidence::SuppliedPair(pair) => pair.target,
            _ => unreachable!(),
        }
    }
    fn packet(
        &self,
        query: InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<NativeTargetPlaybackQuery<()>> {
        let history = self.owner.phase_history().unwrap();
        let mut selected = None;
        for i in 0..history.len() {
            let phase = history.span(i).unwrap();
            let words = phase.to_words();
            let start = TargetTime::new(words[4], words[5], words[6])?;
            let end = history
                .span(i + 1)
                .map(|next| {
                    let words = next.to_words();
                    TargetTime::new(words[4], words[5], words[6]).unwrap()
                })
                .unwrap_or(history.end_time());
            if query.compare_target_time(start).is_lt()
                || query.compare_target_time(end).is_gt()
                || (i + 1 < history.len() && query.compare_target_time(end).is_eq())
            {
                continue;
            }
            selected = Some((phase, end));
        }
        let (phase, segment_end) = selected.ok_or("query outside actual converted history")?;
        let source = self
            .owner
            .converter_owner()
            .source_playback_history()
            .unwrap();
        let playback = (0..source.len())
            .filter_map(|i| source.span(i))
            .find(|span| {
                phase
                    .compare_source_frame_at_interpolated_time(
                        query,
                        segment_end,
                        span.physical_start(),
                    )
                    .is_ok_and(|order| !order.is_lt())
                    && span.physical_end().is_none_or(|end| {
                        phase
                            .compare_source_frame_at_interpolated_time(query, segment_end, end)
                            .is_ok_and(|order| order.is_lt())
                    })
            })
            .ok_or("query outside actual Source history")?;
        Ok(NativeTargetPlaybackQuery::new(
            0,
            self.basis,
            identity,
            query,
            NativeTargetPlaybackFacts {
                playback: ConvertedPlaybackFacts {
                    phase,
                    segment_end,
                    generated_target_end: history.end_time(),
                    submitted_target_end: self.owner.admitted_target_time().unwrap(),
                    playback,
                    pulled_source_end: source.pulled_end_frame(),
                    revision: (self.lap.get() as u64 + 1) * 2,
                },
                source: self.owner.last_real_source_report(),
                converted: Some(self.report),
                boundaries: self.owner.boundaries(),
                revision: (self.lap.get() as u64 + 1) * 4,
            },
            Some(()),
        )?)
    }
}
impl GameplayDevice for Device {
    type Presentation = PresentationEstimator;
    fn observe_coordinate_playback_control(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinatePlaybackControlFacts>> {
        self.lap.set(self.lap.get() + 1);
        let block = self
            .block_schedule
            .get(self.lap.get() - 1)
            .copied()
            .unwrap_or_else(|| {
                if self.lap.get() == 1 {
                    self.first_block
                } else {
                    8
                }
            });
        self.report = self.owner.render_pending(block)?;
        self.owner.admit(block)?;
        let (source, target) = self.owner.mixer().practice_history_retention_pins();
        if let (Some(source), None) = (source, target) {
            self.unprojected_at = Some((self.lap.get(), self.report.target_frame_cursor, source));
        }
        if let (Some(source), Some(target)) = (source, target) {
            self.pins.push((source, target));
        }
        let coordinate = snapshot(self.basis, self.report.target_frame_cursor);
        let query = exact(
            self.basis
                .start_time()
                .checked_add_frames(self.report.target_frame_cursor, 1500)?,
        );
        let observation =
            NativeTargetPlaybackObservation::new(coordinate, self.packet(query, identity)?)?;
        presentation.admit_target_coordinate(coordinate)?;
        Ok(Some(
            TargetCoordinateControlFacts::from_target(&observation, presentation, identity)?.into(),
        ))
    }
    fn playback_mapping_at(
        &self,
        _: &NativeAudioPresentation,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<NativePlaybackMapping>> {
        if self.mapping_missing && self.lap.get() >= 2 {
            return Ok(None);
        }
        let packet = self.packet(*query, identity)?;
        self.mapped
            .borrow_mut()
            .push((*query, packet.facts().playback));
        Ok(Some(NativePlaybackMapping::from_target(&packet)))
    }
    fn observe(&mut self, _: &mut Self::Presentation) -> NativeGameplayResult<()> {
        panic!("scalar observer")
    }
    fn observe_audio(&mut self, _: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        panic!("separate observer")
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(self.owner.last_real_source_report())
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(self.now())
    }
    fn set_audio_held(&mut self, _: bool) -> NativeGameplayResult<()> {
        panic!("practice must not restart output")
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        while self
            .events
            .front()
            .is_some_and(|event| event.meta().timestamp <= self.now().timestamp)
        {
            let event = self.events.pop_front().unwrap();
            self.acquired
                .push((self.lap.get(), self.now(), *event.meta()));
            events.push_back(event);
        }
        Ok(InputBatch {
            completed_through: Some(self.now()),
            backlog: false,
            closed: self.lap.get() >= self.stop_at,
        })
    }
    fn observe_end(
        &mut self,
        _: &mut NativeEnd,
        _: &Self::Presentation,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        panic!("scalar end")
    }
    fn seed_resume(
        &mut self,
        _: &mut Self::Presentation,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        panic!("scalar resume")
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        panic!("scalar schedule")
    }
}

#[derive(Default)]
struct Host {
    generations: Vec<u64>,
    identities: Vec<u64>,
    commits: Vec<u64>,
    replies: Vec<PracticeReply>,
    reports: Vec<RuntimeReport>,
    request: Option<PracticeRequest>,
    refuse_visual: bool,
    transaction_capabilities: Vec<PracticeTransactionCapability>,
    transaction_requests: VecDeque<PracticeTransactionRequest>,
    transaction_replies: Vec<PracticeTransactionReply>,
    transaction_after_reports: usize,
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
        Ok(())
    }
    fn publish_local_reports(
        &mut self,
        _: &[crate::local_runtime::PlayerReport],
    ) -> NativeGameplayResult<()> {
        panic!("solo only")
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
    fn advertise_practice(&mut self, cap: Option<PracticeCapability>) -> NativeGameplayResult<()> {
        if let Some(cap) = cap {
            self.generations.push(cap.generation);
        }
        Ok(())
    }
    fn take_practice_request(&mut self) -> NativeGameplayResult<Option<PracticeRequest>> {
        Ok(if self.generations.is_empty() {
            None
        } else {
            self.request.take()
        })
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
        Ok(
            if active
                && !self.transaction_capabilities.is_empty()
                && self.reports.len() >= self.transaction_after_reports
            {
                self.transaction_requests.pop_front()
            } else {
                None
            },
        )
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
        attempts: &[(
            crate::local_players::PlayerId,
            &crate::practice_session::PreparedPracticeAttempt,
        )],
    ) -> NativeGameplayResult<crate::player::PreparedPracticePresentation> {
        crate::player::prepare_practice_presentation(generation, attempts)
    }
    fn apply_practice_identity(
        &mut self,
        prepared: &mut crate::player::PreparedPracticePresentation,
        generation: u64,
    ) {
        crate::player::apply_practice_identity(prepared, generation);
        self.identities.push(generation);
    }
    fn commit_practice_presentation(
        &mut self,
        prepared: crate::player::PreparedPracticePresentation,
        generation: u64,
    ) -> NativeGameplayResult<()> {
        if self.refuse_visual {
            return Err("literal coordinate practice visual refusal".into());
        }
        crate::player::commit_practice_presentation(prepared, generation)?;
        self.commits.push(generation);
        Ok(())
    }
}
#[derive(Default)]
struct Recording {
    captures: Vec<LiveReplayCapture>,
    paths: Vec<std::path::PathBuf>,
}
impl PracticeRecordingPort for Recording {
    fn archive(
        &mut self,
        player: crate::local_players::PlayerId,
        path: &std::path::Path,
        capture: LiveReplayCapture,
    ) -> NativeGameplayResult<()> {
        assert_eq!(player, crate::local_players::PlayerId(1));
        self.paths.push(path.to_owned());
        self.captures.push(capture);
        Ok(())
    }
}
struct Control;
impl NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        Ok(0)
    }
    fn checked_add(moment: u64, duration: WallDuration) -> Option<u64> {
        moment.checked_add(duration.as_nanos() as u64)
    }
    fn wait(&mut self, _: WallDuration) -> NativeGameplayResult<()> {
        Ok(())
    }
}

struct Rig {
    original: beatkernel_bms::BmsChart,
    device: Device,
    runtime: SoloRuntime,
    presentation: NativeAudioPresentation,
    gauge: BmsGauge,
    bgm: BgmFeeder,
    pause: NativePause,
    capture: Option<LiveReplayCapture>,
    merger: InputMerger,
    playback: PracticePlayback,
    host: Host,
    recording: Recording,
    cfg: CoordinateGameplayConfig,
}
impl Rig {
    fn new(rate: Rate, repeat: bool) -> Self {
        Self::new_mode(rate, repeat, false)
    }
    fn new_runtime(rate: Rate) -> Self {
        Self::new_mode(rate, false, true)
    }
    fn new_mode(rate: Rate, repeat: bool, runtime_rates: bool) -> Self {
        let original = beatkernel_bms::parse(
            "#BPM 3000\n#WAV01 key.wav\n#00011:0001000000000000\n",
            Default::default(),
        )
        .unwrap();
        let policy = ResolvedPlayPolicy::builtin(1_000_000, 1_000_000, 0).unwrap();
        let format = AudioFormat::new(1000, 1).unwrap();
        let bank = SampleBank::new(format, PcmLimits::new(8, 256, 1).unwrap()).unwrap();
        let program_limits = PracticeLimits::new(8, 1, 64, 8, 64).unwrap();
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
        let (producer, consumer) = command_queue(64).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(2),
                Timestamp::ZERO,
                AudioLimits::new(64, 8, 64, 256, 64).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        let end = Timestamp::from_nanos(30_000_000);
        let region = PracticeRegion::new(Timestamp::ZERO, end, repeat).unwrap();
        mixer.install_practice(program, endpoint, region).unwrap();
        mixer.enable_source_playback_history(128).unwrap();
        let source_basis = mixer.output_frame_basis();
        let mut owner = ConvertedNativeOutputState::new(
            mixer,
            DeviceFormat::new(1500, 1, SampleEncoding::Float32, None).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            16,
        )
        .unwrap_or_else(|_| panic!("original converter"));
        owner.enable_phase_history(128).unwrap();
        let basis = owner.target_frame_basis();
        let initial_report = if runtime_rates {
            None
        } else {
            let report = owner.render_pending(8).unwrap();
            owner.admit(8).unwrap();
            Some(report)
        };
        let speed = ReplaySpeed::new(rate).unwrap();
        let setup = LiveSpeedSetup::new(Timestamp::ZERO, Duration::ZERO, speed, Some(end)).unwrap();
        let make_transport = if runtime_rates {
            Transport::new_with_runtime_output_phase
        } else {
            Transport::new_with_output_phase
        };
        let transport = make_transport(
            OutputPhaseBasis::new(ClockDomainId(3), 91, 1, 0, 1000).unwrap(),
            point(3, 0),
            Timestamp::ZERO,
            Duration::ZERO,
            rate,
            Some(end),
        )
        .unwrap();
        let judge = JudgeEngine::new(
            original.compile().unwrap().chart,
            original.rules(),
            policy.judge().clone(),
        )
        .unwrap();
        let capture = Some(
            LiveReplayCapture::new_with_policy_and_speed(
                &judge,
                ClockDomainId(3),
                limits(),
                Timestamp::ZERO,
                0,
                Some(end),
                beatkernel_bms::BmsInputMode::ButtonOnly,
                None,
                &policy,
                speed,
            )
            .unwrap(),
        );
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
            vec![],
            64,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        runtime.set_audio_scope(CommandScope(1));
        runtime.set_song_end(end).unwrap();
        let launch = SessionLaunch::new(vec![
            "--chart".into(),
            "fixture.bms".into(),
            "--record-replay".into(),
            "practice.bkr".into(),
            "--speed".into(),
            speed.to_string(),
        ])
        .unwrap();
        let members = vec![PracticeMember {
            player: crate::local_players::PlayerId(1),
            policy: ResolvedPlayPolicy::builtin(1_000_000, 1_000_000, 0).unwrap(),
            launch,
            chart_seed: 0,
            capture_limits: Some(limits()),
        }];
        let playback = if runtime_rates {
            PracticePlayback::new_with_runtime_rates(
                controller,
                original.clone(),
                members,
                region,
                Timestamp::from_nanos(200_000_000),
                source_basis,
                Some(end),
                setup,
                vec![PracticeRuntimeHeader {
                    player: crate::local_players::PlayerId(1),
                    header: capture.as_ref().unwrap().header().clone(),
                    limits: limits(),
                }],
            )
        } else {
            PracticePlayback::new_with_speed(
                controller,
                original.clone(),
                members,
                region,
                Timestamp::from_nanos(200_000_000),
                source_basis,
                2,
                Some(end),
                setup,
            )
        }
        .unwrap();
        let report = initial_report.unwrap_or_else(|| {
            let report = owner.render_pending(8).unwrap();
            owner.admit(8).unwrap();
            report
        });
        let mut presentation = new_coordinate_audio_presentation(
            0,
            AudioAuthorityCoordinateBasis::TargetFrame(basis),
            ClockDomainId(1),
            point(3, 0),
            AudioAuthorityConfig::default(),
        )
        .unwrap();
        for frame in [0, 2] {
            presentation
                .admit_target_coordinate(snapshot(basis, frame))
                .unwrap();
        }
        let stream = presentation.authority().epoch().stream_origin;
        let cfg = CoordinateGameplayConfig {
            audio: AudioGameplayConfig {
                gameplay: NativeGameplayConfig {
                    origin: point(1, 90),
                    stream_origin: stream,
                    playback_origin: stream,
                    song_origin: Timestamp::ZERO,
                    sample_rate: 1000,
                    end_song: Some(end),
                    advance_lag: Duration::ZERO,
                    seconds: None,
                    pause_supported: true,
                    logical_schedule: true,
                },
                section_start: Timestamp::ZERO,
            },
            source_history: SourcePlaybackIdentity::new(1000, 0, 0).unwrap(),
        };
        Self {
            original,
            device: Device {
                owner,
                report,
                basis,
                lap: Rc::new(Cell::new(0)),
                stop_at: 10,
                mapping_missing: false,
                events: VecDeque::new(),
                pins: vec![],
                first_block: 8,
                block_schedule: &[],
                unprojected_at: None,
                acquired: vec![],
                mapped: RefCell::new(vec![]),
            },
            runtime,
            presentation,
            gauge: BmsGauge::new(policy.gauge().try_copy().unwrap()),
            bgm: BgmFeeder::new(
                vec![],
                BgmConfig {
                    output_origin: point(2, 0),
                    sample_rate: 1000,
                    preroll: Duration::ZERO,
                    lookahead: Duration::from_nanos(1),
                    max_pending: 1,
                },
            )
            .unwrap(),
            pause: NativePause::new(point(2, 0), ClockDomainId(1), 1000)
                .unwrap()
                .with_target_basis(0, basis)
                .unwrap(),
            capture,
            merger: InputMerger::new_dynamic(ClockDomainId(1), point(1, 90), 4096, 64).unwrap(),
            playback,
            host: Host::default(),
            recording: Recording::default(),
            cfg,
        }
    }
    fn input(&mut self, ns: i64, sequence: u64) {
        self.device
            .events
            .push_back(PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(DeviceId(9), point(1, ns), sequence),
                control: PhysicalControlId::keyboard(7u16),
                state: ButtonState::Down,
            }));
    }
    fn scrub(&mut self) {
        self.host.request = Some(PracticeRequest {
            id: 91,
            generation: 1,
            action: PracticeAction::Scrub {
                target: Timestamp::from_nanos(10_000_001),
            },
        });
    }
    fn run(&mut self) -> NativeGameplayResult<Option<CompletedPlayResult>> {
        self.run_with_competition(&mut Some(NoopSoloCompetition))
    }
    fn run_with_competition<C: crate::gameplay_competition::SoloCompetitionPort>(
        &mut self,
        competition: &mut Option<C>,
    ) -> NativeGameplayResult<Option<CompletedPlayResult>> {
        let mut end = None;
        let mut completion = None;
        let mut delivery = InputDeliveryTelemetry::new(64, ClockDomainId(1)).unwrap();
        let mut pre = 0;
        run_gameplay_audio_coordinate_with_practice_and_ports(
            &mut self.device,
            AudioGameplaySession {
                session: GameplaySession {
                    runtime: &mut self.runtime,
                    gauge: &mut self.gauge,
                    bgm: &mut self.bgm,
                    discipline: &mut self.presentation,
                    pause: &mut self.pause,
                    end: &mut end,
                    completion: &mut completion,
                    capture: &mut self.capture,
                    competition,
                    delivery: &mut delivery,
                    pre_origin_inputs: &mut pre,
                },
                merger: &mut self.merger,
            },
            self.cfg,
            &mut Control,
            &mut self.host,
            &mut self.playback,
            &mut self.recording,
        )
    }
    fn assert_phase(&mut self, generation: u64, rate: Rate) {
        let transport = self.runtime.transport_mut();
        let basis = transport.output_phase_basis().unwrap();
        assert_eq!(basis.owner(), 91);
        assert_eq!(basis.domain(), ClockDomainId(3));
        assert_eq!(basis.source_hz(), 1000);
        assert_eq!(basis.generation(), generation);
        assert!(basis.active_origin_frame() > 0);
        assert_eq!(transport.anchor().rate, rate);
        assert_eq!(self.runtime.audio_scope(), CommandScope(generation));
        assert_eq!(self.presentation.authority().epoch().id, 0);
        // target_frame_basis() is the next generated cursor, not the cold
        // stream descriptor. Check the immutable original phase header and
        // admitted authority descriptor instead of expecting that cursor to freeze.
        assert_eq!(
            self.device.owner.phase_history().unwrap().start_time(),
            self.device.basis.start_time()
        );
        assert_eq!(
            self.presentation.authority().coordinate_basis(),
            Some(AudioAuthorityCoordinateBasis::TargetFrame(
                self.device.basis
            ))
        );
        let next = self.device.owner.target_frame_basis();
        assert_eq!(next.origin(), self.device.basis.origin());
        assert_eq!(next.sample_rate(), self.device.basis.sample_rate());
    }
    fn assert_replay_matches_live_judge(&self, capture: LiveReplayCapture) {
        let replay = crate::replay_playback::reconstruct_section(
            &self.original,
            capture.into_file(),
            limits(),
        )
        .unwrap();
        assert_eq!(
            replay.engine().stable_hash().unwrap(),
            self.runtime.judge().stable_hash().unwrap(),
            "the actual fresh attempt must equal its canonical reconstructed replay"
        );
    }
}

#[test]
fn actual_target_solo_loop_replaces_phase_and_keeps_equal_cut_input_in_fresh_capture() {
    let mut f = Rig::new(Rate::NORMAL, true);
    // The genuine first loop Source frame30 projects to Target45 at exactly30ms.
    f.input(30_000_100, 7);
    assert!(f.run().unwrap().is_none());
    assert_eq!(f.playback.generation(), 2);
    f.assert_phase(2, Rate::NORMAL);
    assert_eq!(f.host.identities, [2]);
    assert_eq!(f.host.commits, [2]);
    assert_eq!(
        f.recording.paths,
        [std::path::PathBuf::from("practice.bkr")]
    );
    let old = &f.recording.captures[0];
    assert!(!old
        .records()
        .iter()
        .any(|r| matches!(r.operation, ReplayOperation::Input(_))));
    let fresh = f.capture.take().unwrap();
    let inputs: Vec<_> = fresh
        .records()
        .iter()
        .filter_map(|r| match &r.operation {
            ReplayOperation::Input(input) => Some((r.song_time, input.physical.meta().sequence)),
            _ => None,
        })
        .collect();
    assert_eq!(inputs, [(Timestamp::ZERO, 7)]);
    f.assert_replay_matches_live_judge(fresh);
}

#[test]
fn actual_target_loop_closes_an_old_prefix_without_projecting_a_negative_fresh_phase() {
    for stop_at in [4, 6] {
        let rate = Rate::new(3, 2).unwrap();
        let mut f = Rig::new(rate, true);
        let pristine_judge = f.runtime.judge().stable_hash().unwrap();
        let pristine_gauge = *f.gauge.snapshot();
        f.device.stop_at = stop_at;
        f.cfg.audio.gameplay.advance_lag = Duration::from_nanos(1_000_000);
        assert!(f.device.events.is_empty());
        // Source20 is the real loop cut/first Target30. Genuine Target24/32
        // observations bracket it at16ms/21+1/3ms; acquired20+1/3ms selects A24.
        let original_header = f.capture.as_ref().unwrap().header().clone();
        assert!(f.run().unwrap().is_none());
        f.assert_phase(2, rate);
        assert_eq!(
            f.runtime
                .transport_mut()
                .output_phase_basis()
                .unwrap()
                .active_origin_frame(),
            20
        );
        assert_eq!(f.host.identities, [2]);
        assert_eq!(f.host.commits, [2]);
        assert!(!f.playback.transition_pending());
        assert!(f.playback.peek_receipt().unwrap().is_none());
        assert!(f.device.mapped.borrow().iter().any(|(query, _)| query
            .compare_target_time(TargetTime::from_frames(24, 1500).unwrap())
            .is_eq()));
        if stop_at == 4 {
            assert_eq!(f.runtime.judge().stable_hash().unwrap(), pristine_judge);
            assert_eq!(f.runtime.judge().effective_song_time(), None);
            assert_eq!(*f.gauge.snapshot(), pristine_gauge);
            assert!(f.capture.as_ref().unwrap().records().is_empty());
        }

        assert_eq!(f.recording.captures.len(), 1);
        let old_header = f.recording.captures[0].header().clone();
        assert_eq!(old_header, original_header);
        assert_eq!(
            f.recording.captures[0].records().last().unwrap().song_time,
            Timestamp::from_nanos(30_000_000)
        );
        assert!(f.recording.captures[0]
            .records()
            .iter()
            .all(|record| matches!(record.operation, ReplayOperation::Advance)));

        // In the longer continuous run, original Target40 is genuinely
        // observed and acquired; source26+2/3 minus origin20 is positive.
        if stop_at == 4 {
            continue;
        }
        assert_eq!(f.playback.generation(), 2);
        // Native OutputPhasePoint floors source elapsed to6666666ns before the
        // selected3/2 rate; preserve that existing integer phase contract.
        assert_eq!(
            f.runtime.judge().effective_song_time(),
            Some(Timestamp::from_nanos(9_999_999))
        );
        assert_eq!(
            f.host.reports.last().unwrap().song_time,
            Timestamp::from_nanos(9_999_999)
        );
        assert_eq!(f.host.identities, [2]);
        assert_eq!(f.host.commits, [2]);
        assert_eq!(f.recording.captures.len(), 1);
        assert_eq!(f.recording.captures[0].header(), &old_header);
        let fresh = f.capture.take().unwrap();
        let setup = crate::replay_playback::decode_section_setup(&fresh.header().options).unwrap();
        assert_eq!(setup.start, Timestamp::ZERO);
        assert_eq!(setup.source_speed.rate(), rate);
        assert_eq!(
            fresh.records().last().unwrap().song_time,
            Timestamp::from_nanos(9_999_999)
        );
        f.assert_replay_matches_live_judge(fresh);
    }
}

#[test]
fn actual_target_solo_scrub_preserves_selected_speed_and_original_fractional_cut() {
    let rate = Rate::new(3, 2).unwrap();
    let mut f = Rig::new(rate, false);
    // Generate 8+7 original Target frames before the queued request. Linear
    // lookahead has pulled through Source11, whose first Target crossing is17
    // (Source11+1/3), rather than the divisible Source12 cut from 8+8.
    f.device.first_block = 7;
    f.scrub();
    f.input(11_333_434, 7);
    f.input(20_000_100, 8);
    f.run().unwrap();
    f.assert_phase(2, rate);
    assert_eq!(f.host.identities, [2]);
    assert_eq!(f.host.replies.len(), 1);
    assert!(f.host.replies[0].result.is_ok());
    assert_eq!(f.recording.captures.len(), 1);
    let fresh = f.capture.take().unwrap();
    assert_eq!(
        crate::replay_playback::decode_section_setup(&fresh.header().options)
            .unwrap()
            .source_speed,
        ReplaySpeed::new(rate).unwrap()
    );
    assert!(fresh
        .records()
        .iter()
        .any(|r| matches!(r.operation, ReplayOperation::Input(_))));
    let active_cut = f
        .runtime
        .transport_mut()
        .output_phase_basis()
        .unwrap()
        .active_origin_frame();
    assert_eq!(
        active_cut, 11,
        "the cut belongs to the genuine queued request"
    );
    let projected_cut = TargetTime::from_frames(17, 1500).unwrap();
    let mapped = f.device.mapped.borrow();
    let (_, cut_facts) = mapped
        .iter()
        .find(|(query, _)| query.compare_target_time(projected_cut).is_eq())
        .expect("actual practice service must query the projected Target17 cut");
    assert!(cut_facts
        .phase
        .compare_source_frame_at_interpolated_time(
            exact(projected_cut),
            cut_facts.segment_end,
            active_cut
        )
        .unwrap()
        .is_gt());
    assert!(cut_facts
        .phase
        .compare_source_frame_at_interpolated_time(
            exact(projected_cut),
            cut_facts.segment_end,
            active_cut + 1
        )
        .unwrap()
        .is_lt());
    assert_eq!(
        cut_facts
            .phase
            .playback_elapsed_nanos_at_interpolated_time(
                exact(projected_cut),
                cut_facts.segment_end,
                cut_facts.playback,
                active_cut
            )
            .unwrap(),
        333_333
    );
    let first = fresh
        .records()
        .first()
        .expect("the equal-cut input must enter the fresh attempt");
    assert!(
        matches!(&first.operation, ReplayOperation::Input(input) if input.physical.meta().sequence == 7)
    );
    // HOST crossing rounds upward from the original frame15/frame23 pair. The
    // retained equal-cut input is just after the exact Target cut and preserves
    // its Source fraction; starting the new attempt at artificial zero fails.
    let elapsed = mapped
        .iter()
        .filter(|(query, _)| query.compare_target_time(projected_cut).is_gt())
        .filter_map(|(query, facts)| {
            facts
                .phase
                .playback_elapsed_nanos_at_interpolated_time(
                    *query,
                    facts.segment_end,
                    facts.playback,
                    active_cut,
                )
                .ok()
        })
        .find(|elapsed| (333_333..334_000).contains(elapsed))
        .expect("actual equal-cut input mapping retains the fractional source position");
    assert_eq!(
        first.song_time,
        Timestamp::from_nanos(10_000_001 + (elapsed * 3 / 2) as i64)
    );
    assert!(first.song_time > Timestamp::from_nanos(10_000_001));
    drop(mapped);
    f.assert_replay_matches_live_judge(fresh);
}

#[test]
fn missing_actual_target_mapping_waits_without_archive_fresh_scope_or_success_ack() {
    let mut f = Rig::new(Rate::NORMAL, true);
    f.device.mapping_missing = true;
    f.run().unwrap();
    assert_eq!(f.playback.generation(), 1);
    assert_eq!(f.runtime.audio_scope(), CommandScope(1));
    assert_eq!(
        f.runtime
            .transport_mut()
            .output_phase_basis()
            .unwrap()
            .generation(),
        1
    );
    assert!(f.recording.captures.is_empty());
    assert!(f.host.identities.is_empty());
    assert!(f.host.commits.is_empty());
    assert!(f.host.replies.is_empty());
    assert!(f.capture.is_some());
}

#[test]
fn actual_target_visual_refusal_keeps_new_speed_phase_and_capture_identity() {
    let rate = Rate::new(3, 2).unwrap();
    let mut f = Rig::new(rate, true);
    f.host.refuse_visual = true;
    assert_eq!(
        f.run().unwrap_err().to_string(),
        "literal coordinate practice visual refusal"
    );
    assert_eq!(f.playback.generation(), 2);
    f.assert_phase(2, rate);
    assert_eq!(f.host.identities, [2]);
    assert!(f.host.commits.is_empty());
    assert_eq!(f.recording.captures.len(), 1);
    let fresh = f.capture.take().unwrap();
    assert_eq!(
        crate::replay_playback::decode_section_setup(&fresh.header().options)
            .unwrap()
            .source_speed,
        ReplaySpeed::new(rate).unwrap()
    );
    assert!(fresh.records().is_empty());
    f.assert_replay_matches_live_judge(fresh);
}

fn request_runtime_rate(f: &mut Rig, requested_rate: Rate) {
    f.host
        .transaction_requests
        .push_back(PracticeTransactionRequest::Rate {
            id: 14102,
            generation: f.playback.generation(),
            expected: f.playback.rate_state().unwrap(),
            requested_rate,
        });
}
fn applied_runtime_rate(f: &Rig) -> PracticeRateEvent {
    assert_eq!(f.host.transaction_replies.len(), 1);
    match &f.host.transaction_replies[0] {
        PracticeTransactionReply::Rate {
            id: 14102,
            generation: 1,
            result: Ok(event),
        } => *event,
        reply => panic!("actual rate result missing: {reply:?}"),
    }
}
#[test]
fn runtime_target_rate_uses_actual_native_cut_without_replacing_the_attempt() {
    let initial = Rate::NORMAL;
    let changed = Rate::new(3, 2).unwrap();
    let mut f = Rig::new_runtime(initial);
    // lap1 publishes the10ms input; lap2 queues after Source has pulled17.
    // lap3 presents Target32 and commits cut17/projected Target26. lap4 closes
    // before another owner event is applied, preserving this attempt's result.
    f.device.stop_at = 4;
    f.host.transaction_after_reports = 1;
    f.input(10_000_100, 7);
    request_runtime_rate(&mut f, changed);
    f.run().unwrap();
    let event = applied_runtime_rate(&f);
    assert_eq!(event.outcome, PracticeRateOutcome::Applied);
    assert_eq!(event.previous_rate, initial);
    assert_eq!(event.applied_rate, changed);
    assert_eq!(f.playback.generation(), 1);
    assert_eq!(f.playback.current_speed().rate(), changed);
    assert_eq!(f.playback.speed().rate(), initial);
    assert_eq!(f.runtime.audio_scope(), CommandScope(1));
    assert!(
        f.host.identities.is_empty(),
        "rate must preserve score/held identity"
    );
    assert!(f.host.commits.is_empty());
    assert!(f.recording.captures.is_empty());
    assert_eq!(
        f.runtime
            .transport_mut()
            .output_phase_basis()
            .unwrap()
            .generation(),
        1
    );
    let effective = f
        .playback
        .effective_header(crate::local_players::PlayerId(1))
        .unwrap();
    let (_, schedule) = crate::replay_speed_policy::split_schedule(&effective.options).unwrap();
    assert_eq!(schedule.current_speed().rate(), changed);
    assert_eq!(schedule.cuts().len(), 1);
    assert_eq!(f.capture.as_ref().unwrap().header(), effective);
    let inputs: Vec<_> = f
        .capture
        .as_ref()
        .unwrap()
        .records()
        .iter()
        .filter_map(|r| match &r.operation {
            ReplayOperation::Input(input) => Some(input.physical.meta()),
            _ => None,
        })
        .collect();
    assert_eq!(
        inputs.len(),
        1,
        "the original input remains in the same recording"
    );
    assert_eq!(inputs[0].sequence, 7);
    assert_eq!(inputs[0].original_clock_point, Some(point(1, 10_000_100)));
    let capture = f.capture.take().unwrap();
    f.assert_replay_matches_live_judge(capture);
}

#[test]
fn runtime_target_input_between_source_cut_and_first_projected_sample_uses_new_rate() {
    let mut f = Rig::new_runtime(Rate::NORMAL);
    f.device.stop_at = 4;
    f.host.transaction_after_reports = 1;
    f.input(10_000_100, 7);
    // Actual Source17 is17ms; its first generated Target sample is26/1500s.
    // This physical release lies strictly between those two boundaries.
    let release_host = point(1, 17_100_100);
    f.device.events.push_back(PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), release_host, 8),
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Up,
    }));
    let changed = Rate::new(3, 2).unwrap();
    request_runtime_rate(&mut f, changed);
    assert!(f.run().unwrap().is_none());

    let event = applied_runtime_rate(&f);
    assert_eq!(event.outcome, PracticeRateOutcome::Applied);
    assert_eq!(event.physical_frame, 17);
    assert_eq!(event.active_elapsed_nanos, 17_000_000);
    let projected = TargetTime::from_frames(26, 1500).unwrap();
    assert!(f.device.pins.iter().any(|(source, target)| {
        *source == 17 && *target == projected
    }), "actual queued event must retain its Source17/Target26 projection");
    assert_eq!(f.playback.generation(), 1);
    assert_eq!(f.runtime.audio_scope(), CommandScope(1));
    assert_eq!(f.playback.current_speed().rate(), changed);
    assert!(f.host.identities.is_empty());
    assert!(f.host.commits.is_empty());
    assert!(f.recording.captures.is_empty());

    let header = f.playback.effective_header(crate::local_players::PlayerId(1)).unwrap();
    let (_, schedule) = crate::replay_speed_policy::split_schedule(&header.options).unwrap();
    assert_eq!(schedule.cuts().len(), 1);
    let cut = schedule.cuts()[0];
    assert_eq!((cut.whole(), cut.numerator(), cut.denominator()), (17_000_000, 0, 1));
    assert_eq!(schedule.current_speed().rate(), changed);
    let capture = f.capture.as_ref().unwrap();
    assert_eq!(capture.header(), header);
    let inputs: Vec<_> = capture.records().iter().filter_map(|record| match &record.operation {
        ReplayOperation::Input(input) => Some((record.song_time, input)),
        _ => None,
    }).collect();
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0].1.physical.meta().sequence, 7);
    assert_eq!(inputs[0].1.physical.meta().original_clock_point, Some(point(1, 10_000_100)));
    let (song, release) = inputs[1];
    assert_eq!(song, Timestamp::from_nanos(17_150_000), "old-rate drain would incorrectly record17_100_000");
    assert_eq!(release.physical.meta().sequence, 8);
    assert_eq!(release.physical.meta().source, DeviceId(9));
    assert_eq!(release.physical.meta().original_clock_point, Some(release_host));
    assert!(matches!(&release.physical, PhysicalInputEvent::Button(button)
        if button.control == PhysicalControlId::keyboard(7u16) && button.state == ButtonState::Up));
    let capture = f.capture.take().unwrap();
    f.assert_replay_matches_live_judge(capture);
}
#[test]
fn runtime_target_noop_and_stale_requests_preserve_phase_header_and_attempt() {
    for stale in [false, true] {
        let mut f = Rig::new_runtime(Rate::NORMAL);
        // target32 (initial8 + three blocks of8) is21.333ms at1500Hz,
        // after the control decision and before the finite30ms Ended event.
        f.device.stop_at = 3;
        let before_header = f.capture.as_ref().unwrap().header().clone();
        request_runtime_rate(
            &mut f,
            if stale {
                Rate::new(3, 2).unwrap()
            } else {
                Rate::NORMAL
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
        assert_eq!(f.playback.generation(), 1);
        assert_eq!(f.playback.current_speed().rate(), Rate::NORMAL);
        assert_eq!(f.runtime.transport_mut().anchor().rate, Rate::NORMAL);
        assert_eq!(f.capture.as_ref().unwrap().header(), &before_header);
        assert_eq!(
            f.playback
                .effective_header(crate::local_players::PlayerId(1)),
            Some(&before_header)
        );
        assert!(f.host.identities.is_empty());
        assert!(f.recording.captures.is_empty());
        assert_eq!(f.host.transaction_replies.len(), 1);
        match &f.host.transaction_replies[0] {
            PracticeTransactionReply::Rate { result: Err(_), .. } if stale => {}
            PracticeTransactionReply::Rate {
                result: Ok(event), ..
            } if !stale => {
                assert_eq!(event.outcome, PracticeRateOutcome::Noop);
                assert_eq!(event.applied_rate, Rate::NORMAL);
            }
            reply => panic!("unexpected runtime refusal/noop: {reply:?}"),
        }
    }
}

#[test]
fn runtime_target_unprojected_source_cut_holds_acquired_input_until_actual_projection() {
    let mut f = Rig::new_runtime(Rate::NORMAL);
    // Initial8, then16/24/26/34/36. At exclusive26 the actual Source17
    // rate exists, but Target26 has not been generated. The last short block
    // closes before finite Ended at Source26/projected Target39 can be observed.
    f.device.block_schedule = &[8, 8, 2, 8, 2];
    f.device.stop_at = 5;
    f.host.transaction_after_reports = 1;
    f.input(10_000_100, 7);
    let release_host = point(1, 17_100_100);
    f.device
        .events
        .push_back(PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(9), release_host, 8),
            control: PhysicalControlId::keyboard(7u16),
            state: ButtonState::Up,
        }));
    let changed = Rate::new(3, 2).unwrap();
    request_runtime_rate(&mut f, changed);
    assert!(f.run().unwrap().is_none());

    assert_eq!(f.device.unprojected_at, Some((3, 26, 17)));
    let acquired = f
        .device
        .acquired
        .iter()
        .filter(|(_, _, meta)| meta.sequence == 8)
        .collect::<Vec<_>>();
    assert_eq!(acquired.len(), 1, "the physical release is acquired once");
    let (lap, completed_through, physical_meta) = *acquired[0];
    assert_eq!(lap, 3);
    assert_eq!(completed_through, point(1, 17_333_433));
    assert_eq!(physical_meta, EventMeta::new(DeviceId(9), release_host, 8));

    let event = applied_runtime_rate(&f);
    assert_eq!(event.outcome, PracticeRateOutcome::Applied);
    assert_eq!(event.physical_frame, 17);
    assert_eq!(event.active_elapsed_nanos, 17_000_000);
    let projected = TargetTime::from_frames(26, 1500).unwrap();
    assert!(
        f.device
            .pins
            .iter()
            .any(|(source, target)| *source == 17 && *target == projected)
    );
    assert_eq!(f.playback.generation(), 1);
    assert_eq!(f.runtime.audio_scope(), CommandScope(1));
    assert_eq!(f.playback.current_speed().rate(), changed);
    assert_eq!(
        f.runtime.transport_mut().runtime_output_rate(),
        Some(changed)
    );
    assert!(f.host.identities.is_empty());
    assert!(f.host.commits.is_empty());
    assert!(f.recording.captures.is_empty());
    assert!(!f.device.owner.mixer().practice_finished());

    let header = f
        .playback
        .effective_header(crate::local_players::PlayerId(1))
        .unwrap();
    let (_, schedule) = crate::replay_speed_policy::split_schedule(&header.options).unwrap();
    assert_eq!(schedule.cuts().len(), 1);
    let cut = schedule.cuts()[0];
    assert_eq!(
        (cut.whole(), cut.numerator(), cut.denominator()),
        (17_000_000, 0, 1)
    );
    assert_eq!(schedule.current_speed().rate(), changed);
    let capture = f.capture.as_ref().unwrap();
    assert_eq!(capture.header(), header);
    let inputs = capture
        .records()
        .iter()
        .filter_map(|record| match &record.operation {
            ReplayOperation::Input(input) => Some((record.song_time, input)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0].1.physical.meta().sequence, 7);
    assert_eq!(
        inputs[0].1.physical.meta().original_clock_point,
        Some(point(1, 10_000_100))
    );
    let (song, release) = inputs[1];
    assert_eq!(
        song,
        Timestamp::from_nanos(17_150_000),
        "old-rate processing before projection would record17_100_000"
    );
    assert_eq!(release.physical.meta().sequence, 8);
    assert_eq!(release.physical.meta().source, DeviceId(9));
    assert_eq!(
        release.physical.meta().original_clock_point,
        Some(release_host)
    );
    assert!(
        matches!(&release.physical, PhysicalInputEvent::Button(button)
        if button.control == PhysicalControlId::keyboard(7u16) && button.state == ButtonState::Up)
    );
    let capture = f.capture.take().unwrap();
    f.assert_replay_matches_live_judge(capture);
}
