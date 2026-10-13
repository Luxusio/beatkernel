//! Actual solo Source-grid practice with genuine Mixer receipts; DI is not hardware proof.
use super::*;
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    bgm::BgmConfig,
    gameplay::output::ports::{
        NativePlaybackObservation, NativeSourcePlaybackQuery, SourcePlaybackFacts,
        SourcePlaybackIdentity,
    },
    gameplay_competition::NoopSoloCompetition,
    gameplay_presentation::GameplayDevice,
    native_audio_presentation::NativeAudioSnapshot,
    native_audio_startup::new_coordinate_audio_presentation,
    native_coordinate_control::CoordinateControlFacts,
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
    native_playback_phase::NativePlaybackMapping,
    play_policy::ResolvedPlayPolicy,
    practice_control::{PracticeAction, PracticeCapability, PracticeReply, PracticeRequest},
    practice_playback::{PracticeMember, PracticePlayback, PracticeRecordingPort},
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
    time::{ClockMappingQuality, presentation::PresentationEstimator},
    transport::{OutputPhaseBasis, Rate, Transport},
};
use beatkernel_platform::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters, presentation::validation::OriginalNativePresentationEvidence,
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
fn snapshot(
    basis: OutputFrameBasis,
    frame: u64,
    counter: bool,
    fractional: bool,
) -> CoordinateNativeAudioSnapshot {
    let ticks = frame * 3 + u64::from(counter && fractional);
    let at = point(
        1,
        100 + if counter {
            (u128::from(ticks) * 1_000_000_000 / 3000) as i64
        } else {
            frame as i64 * 1_000_000
        },
    );
    let (coordinate, evidence) = if counter {
        (
            NativeOutputCoordinate::Counter {
                position: ticks,
                frequency: 3000,
            },
            OriginalNativePresentationEvidence::Wasapi {
                basis: Some(basis),
                snapshot: AudioStreamSnapshot {
                    telemetry_available: true,
                    status: AudioStreamStatus::Running,
                    counters: StreamCounters::default(),
                    render: None,
                    clock: Some(AudioClockSnapshot {
                        position: ticks,
                        frequency: 3000,
                        qpc_100ns: ticks,
                        reading_quality: AudioClockReadingQuality::Accurate,
                        host_point: Some(at),
                        mapping_quality: ClockMappingQuality::Unknown,
                    }),
                },
            },
        )
    } else {
        (
            NativeOutputCoordinate::StreamFrame { frame },
            OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(frame).unwrap(),
                target: at,
            }),
        )
    };
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: 0,
            basis,
            evidence,
        },
        coordinate,
    )
    .unwrap()
}
struct Device {
    mixer: Mixer,
    report: RenderReport,
    basis: OutputFrameBasis,
    lap: Rc<Cell<usize>>,
    stop_at: usize,
    mapping_missing: bool,
    missing_boundary_seen: Cell<bool>,
    events: VecDeque<PhysicalInputEvent>,
    pins: Vec<(u64, TargetTime)>,
    first_block: usize,
    counter: bool,
    mapped: RefCell<Vec<(InterpolatedTargetTime, SourcePlaybackFacts)>>,
}
impl Device {
    fn coordinate(&self) -> CoordinateNativeAudioSnapshot {
        let end = self.report.start_frame + self.report.frames as u64;
        snapshot(self.basis, end - 1, self.counter, true)
    }
    fn now(&self) -> ClockPoint {
        match self.coordinate().snapshot().evidence {
            OriginalNativePresentationEvidence::SuppliedPair(pair) => pair.target,
            OriginalNativePresentationEvidence::Wasapi { snapshot, .. } => {
                snapshot.clock.unwrap().host_point.unwrap()
            }
            _ => unreachable!(),
        }
    }
    fn packet(
        &self,
        query: InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<NativeSourcePlaybackQuery<()>> {
        let history = self.mixer.source_playback_history().unwrap();
        let playback = history
            .lookup_at_source_time(query)
            .map_err(|error| format!("Source practice lookup: {error:?}"))?;
        Ok(NativeSourcePlaybackQuery::new(
            0,
            self.basis,
            identity,
            query,
            SourcePlaybackFacts {
                playback,
                pulled_source_end: history.pulled_end_frame(),
                submitted_source_end: history.pulled_end_frame(),
                render: Some(self.report),
                revision: (self.lap.get() as u64 + 1) * 2,
            },
            Some(()),
        )?)
    }
}
impl GameplayDevice for Device {
    type Presentation = PresentationEstimator;
    fn observe_coordinate_control(
        &mut self,
        p: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinateControlFacts>> {
        self.lap.set(self.lap.get() + 1);
        let block = if self.lap.get() == 1 {
            self.first_block
        } else {
            5
        };
        self.report = self.mixer.render(&mut vec![0.0; block])?;
        let (source, target) = self.mixer.practice_history_retention_pins();
        if let (Some(source), Some(target)) = (source, target) {
            self.pins.push((source, target));
        }
        let coordinate = self.coordinate();
        let query = match coordinate.coordinate() {
            NativeOutputCoordinate::StreamFrame { frame } => {
                exact(TargetTime::from_frames(frame, 1000)?)
            }
            NativeOutputCoordinate::Counter {
                position,
                frequency,
            } => exact(TargetTime::from_frames(position, frequency as u32)?),
        };
        let observation =
            NativePlaybackObservation::new(coordinate, self.packet(query, identity)?)?;
        p.admit_coordinate(coordinate)?;
        Ok(Some(CoordinateControlFacts::from_source(
            &observation,
            p,
            identity,
        )?))
    }
    fn playback_mapping_at(
        &self,
        _: &NativeAudioPresentation,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<NativePlaybackMapping>> {
        if self.mapping_missing && self.lap.get() >= 2 {
            if query
                .compare_target_time(TargetTime::from_frames(30, 1000)?)
                .is_eq()
            {
                self.missing_boundary_seen.set(true);
            }
            return Ok(None);
        }
        let packet = self.packet(*query, identity)?;
        self.mapped.borrow_mut().push((*query, packet.facts()));
        Ok(Some(NativePlaybackMapping::from_source(&packet)))
    }
    fn observe(&mut self, _: &mut Self::Presentation) -> NativeGameplayResult<()> {
        panic!("scalar observer")
    }
    fn observe_audio(&mut self, _: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        panic!("separate observer")
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(Some(self.report))
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(self.now())
    }
    fn set_audio_held(&mut self, _: bool) -> NativeGameplayResult<()> {
        panic!("practice cannot restart output")
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
            events.push_back(self.events.pop_front().unwrap());
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
    fn new(rate: Rate, repeat: bool, counter: bool) -> Self {
        let original = beatkernel_bms::parse(
            "#BPM 3000\n#WAV01 key.wav\n#00011:0001000000000000\n",
            Default::default(),
        )
        .unwrap();
        let policy = ResolvedPlayPolicy::builtin(1_000_000, 1_000_000, 0).unwrap();
        let format = AudioFormat::new(1000, 1).unwrap();
        let bank = SampleBank::new(format, PcmLimits::new(8, 256, 1).unwrap()).unwrap();
        let program = PreparedPracticeProgram::new_with_speed(
            &bank,
            vec![],
            PracticeLimits::new(8, 1, 64, 8, 64).unwrap(),
            rate,
            Duration::ZERO,
            false,
        )
        .unwrap();
        let (controller, endpoint) = practice_queue(&program).unwrap();
        let (mut producer, consumer) = if counter {
            command_queue_with_start_gate(64).unwrap()
        } else {
            command_queue(64).unwrap()
        };
        if counter {
            // The real gate delays Started until physical1, retaining original
            // history0/0 and playback0 without admitting a zero native counter.
            producer.schedule_start_at(1).unwrap();
        }
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
        let basis = mixer.output_frame_basis();
        let report = mixer.render(&mut [0.0; 5]).unwrap();
        let descriptor = if counter {
            AudioAuthorityCoordinateBasis::SourceCounter {
                basis,
                frequency: 3000,
            }
        } else {
            AudioAuthorityCoordinateBasis::SourceFrame(basis)
        };
        let mut presentation = new_coordinate_audio_presentation(
            0,
            descriptor,
            ClockDomainId(1),
            point(3, 0),
            AudioAuthorityConfig::default(),
        )
        .unwrap();
        for frame in [0, 2] {
            presentation
                .admit_coordinate(snapshot(basis, frame, counter, counter && frame == 0))
                .unwrap();
        }
        let speed = ReplaySpeed::new(rate).unwrap();
        let setup = LiveSpeedSetup::new(Timestamp::ZERO, Duration::ZERO, speed, Some(end)).unwrap();
        let transport = Transport::new_with_output_phase(
            OutputPhaseBasis::new(ClockDomainId(3), 91, 1, 0, 1000).unwrap(),
            point(3, if counter { 1_000_000 } else { 0 }),
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
        let playback = PracticePlayback::new_with_speed(
            controller,
            original.clone(),
            vec![PracticeMember {
                player: crate::local_players::PlayerId(1),
                policy: ResolvedPlayPolicy::builtin(1_000_000, 1_000_000, 0).unwrap(),
                launch,
                chart_seed: 0,
                capture_limits: Some(limits()),
            }],
            region,
            Timestamp::from_nanos(200_000_000),
            basis,
            2,
            Some(end),
            setup,
        )
        .unwrap();
        let stream = presentation.authority().epoch().stream_origin;
        let cfg = CoordinateGameplayConfig {
            audio: AudioGameplayConfig {
                gameplay: NativeGameplayConfig {
                    origin: point(1, 90),
                    stream_origin: stream,
                    playback_origin: if counter { point(2, 1_000_000) } else { stream },
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
                mixer,
                report,
                basis,
                lap: Rc::new(Cell::new(0)),
                stop_at: 6,
                mapping_missing: false,
                missing_boundary_seen: Cell::new(false),
                events: VecDeque::new(),
                pins: vec![],
                first_block: 5,
                counter,
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
                .with_start_frame(u64::from(counter))
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
        let mut end = None;
        let mut completion = None;
        let mut competition = Some(NoopSoloCompetition);
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
                    competition: &mut competition,
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
        assert_eq!(self.presentation.basis(), Some(self.device.basis));
        assert!(self.presentation.target_basis().is_none());
        assert_eq!(
            self.device
                .mixer
                .source_playback_history()
                .unwrap()
                .initial_physical_frame(),
            0
        );
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

fn assert_fresh_capture(f: &mut Rig, rate: Rate, sequence: u64, exact_start: bool) {
    assert_eq!(f.playback.generation(), 2);
    f.assert_phase(2, rate);
    assert_eq!(f.host.identities, [2]);
    assert_eq!(f.host.commits, [2]);
    assert_eq!(f.recording.captures.len(), 1);
    assert!(!f.recording.captures[0]
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
    assert_eq!(inputs.len(), 1);
    assert_eq!(
        inputs[0].1, sequence,
        "equal-cut input belongs to fresh capture once"
    );
    let setup = crate::replay_playback::decode_section_setup(&fresh.header().options).unwrap();
    assert_eq!(setup.source_speed, ReplaySpeed::new(rate).unwrap());
    if exact_start {
        assert_eq!(inputs[0].0, setup.start);
    }
    f.assert_replay_matches_live_judge(fresh);
}
#[test]
fn actual_source_frame_normal_loop_keeps_equal_cut_input_and_canonical_replay() {
    let mut f = Rig::new(Rate::NORMAL, true, false);
    f.device.stop_at = 7;
    f.input(30_000_100, 7);
    assert!(f.run().unwrap().is_none());
    assert_fresh_capture(&mut f, Rate::NORMAL, 7, true);
    assert_eq!(
        f.runtime
            .transport_mut()
            .output_phase_basis()
            .unwrap()
            .active_origin_frame(),
        30
    );
}
#[test]
fn actual_source_frame_three_halves_loop_keeps_selected_rate_and_replay() {
    let rate = Rate::new(3, 2).unwrap();
    let mut f = Rig::new(rate, true, false);
    f.input(20_000_100, 7);
    f.run().unwrap();
    assert_fresh_capture(&mut f, rate, 7, true);
    assert_eq!(
        f.runtime
            .transport_mut()
            .output_phase_basis()
            .unwrap()
            .active_origin_frame(),
        20
    );
}
#[test]
fn actual_source_frame_scrub_preserves_selected_speed_and_exact_cut() {
    let rate = Rate::new(3, 2).unwrap();
    let mut f = Rig::new(rate, false, false);
    f.device.stop_at = 4;
    f.scrub();
    f.input(10_000_100, 7);
    f.run().unwrap();
    assert_fresh_capture(&mut f, rate, 7, true);
    assert_eq!(
        f.runtime
            .transport_mut()
            .output_phase_basis()
            .unwrap()
            .active_origin_frame(),
        10
    );
    assert_eq!(f.host.replies.len(), 1);
    assert!(f.host.replies[0].result.is_ok());
    assert!(f
        .device
        .mapped
        .borrow()
        .iter()
        .any(|(q, _)| q.compare_source_time_frame(10, 1000).unwrap().is_eq()));
}
#[test]
fn actual_source_counter_fraction_scrub_uses_source_frame_cut_and_original_counter_history() {
    let rate = Rate::new(3, 2).unwrap();
    let mut f = Rig::new(rate, false, true);
    f.device.stop_at = 4;
    f.scrub();
    f.input(10_000_100, 7);
    f.run().unwrap();
    assert_fresh_capture(&mut f, rate, 7, false);
    assert_eq!(
        f.runtime
            .transport_mut()
            .output_phase_basis()
            .unwrap()
            .active_origin_frame(),
        9
    );
    assert_eq!(
        f.presentation.authority().coordinate_basis(),
        Some(AudioAuthorityCoordinateBasis::SourceCounter {
            basis: f.device.basis,
            frequency: 3000
        })
    );
    assert!(
        matches!(f.presentation.authority().latest_coordinate_observation().unwrap().coordinate(), Some(NativeOutputCoordinate::Counter { position, frequency: 3000 }) if position % 3 == 1)
    );
    assert!(f
        .device
        .mapped
        .borrow()
        .iter()
        .any(|(q, _)| q.compare_source_time_frame(10, 1000).unwrap().is_eq()));
    assert_eq!(f.host.replies.len(), 1);
    assert!(f.host.replies[0].result.is_ok());
}
#[test]
fn missing_actual_source_packet_waits_before_archive_scope_and_success_ack() {
    let mut f = Rig::new(Rate::NORMAL, true, false);
    f.device.stop_at = 7;
    f.device.mapping_missing = true;
    f.run().unwrap();
    assert!(f.device.missing_boundary_seen.get());
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
fn actual_source_visual_refusal_retains_fresh_phase_and_capture_identity() {
    let rate = Rate::new(3, 2).unwrap();
    let mut f = Rig::new(rate, true, false);
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
    assert!(fresh.records().is_empty());
    assert_eq!(
        crate::replay_playback::decode_section_setup(&fresh.header().options)
            .unwrap()
            .source_speed,
        ReplaySpeed::new(rate).unwrap()
    );
    f.assert_replay_matches_live_judge(fresh);
}
