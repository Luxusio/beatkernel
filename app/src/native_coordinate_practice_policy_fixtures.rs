//! Selected coordinate-practice admission through genuine mixer/converter and static DI ports.
//! This fixture does not establish ambient UI or acoustic hardware success.
use super::*;
use crate::{
    bgm::BgmFeeder,
    live_speed::LiveSpeedSetup,
    local_input::InputMerger,
    local_runtime::RuntimeGroup,
    native_cohort::{
        AudioCohortSession, CohortSession, GameplayPlayerState,
        run_cohort_audio_coordinate_practice_with_policies_and_results_and_ports,
    },
    native_gameplay::{AudioGameplayConfig, CoordinateGameplayConfig},
    playback_pause::NativePause,
    play_result::CompletedPlayResult,
    replay_capture::LiveReplayCapture,
};
use beatkernel::{
    telemetry::InputDeliveryTelemetry,
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use std::{collections::VecDeque, path::Path, time::Duration as WallDuration};
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    bgm::BgmConfig,
    gameplay::output::ports::{
        NativeTargetPlaybackFacts, NativeTargetPlaybackObservation, NativeTargetPlaybackQuery,
        SourcePlaybackIdentity,
    },
    gameplay_competition::{NoopGroupCompetition, NoopSoloCompetition},
    gameplay_presentation::GameplayDevice,
    local_runtime::MemberConfig,
    native_audio_presentation::{NativeAudioPresentation, TargetNativeAudioSnapshot},
    native_audio_startup::new_coordinate_audio_presentation,
    native_coordinate_control::{CoordinatePlaybackControlFacts, TargetCoordinateControlFacts},
    native_gameplay::InputBatch,
    native_output_coordinate::{CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate},
    native_playback_phase::{ConvertedPlaybackFacts, NativePlaybackMapping},
    practice_control::{PracticeAction, PracticeCapability, PracticeReply, PracticeRequest},
    practice_playback::{PracticeMember, PracticePlayback, PracticeRecordingPort},
    practice_session::{prepare_attempt_with_speed, PracticeAttemptConfig},
    replay_speed::ReplaySpeed,
    session_launch::SessionLaunch,
};
use beatkernel::{
    audio::*,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId,
    },
    replay::{codec::ReplayCodecLimits, ReplaySession},
    runtime::{RuntimeProcessingClock, RuntimeReport},
    time::{presentation::PresentationEstimator, ClockPair, Duration, ExtrapolationPolicy},
    transport::{OutputPhaseBasis, Rate},
};
use beatkernel_platform::audio::{
    ConvertedNativeOutputState, DeviceFormat, SampleEncoding,
    presentation::validation::OriginalNativePresentationEvidence,
};
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
    effects: Rc<Cell<usize>>,
    output: ConvertedNativeOutputState,
    basis: TargetFrameBasis,
    converted: ConvertedRenderReport,
    step: Rc<Cell<usize>>,
    frame: u64,
    generated: u64,
    last_acquired_frame: Option<u64>,
    close_at: usize,
    unavailable: bool,
    scalar_calls: usize,
    queries: RefCell<Vec<InterpolatedTargetTime>>,
    ids: Vec<u32>,
}
impl Device {
    fn observation(&self) -> CoordinateTargetNativeAudioSnapshot {
        CoordinateTargetNativeAudioSnapshot::new(
            TargetNativeAudioSnapshot {
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
    fn packet(
        &self,
        q: InterpolatedTargetTime,
        id: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<NativeTargetPlaybackQuery<OriginalNativePresentationEvidence>> {
        let history = self.output.phase_history().unwrap();
        let index = (0..history.len())
            .filter(|&i| {
                let w = history.span(i).unwrap().to_words();
                !q.compare_target_time(TargetTime::new(w[4], w[5], w[6]).unwrap())
                    .is_lt()
            })
            .last()
            .ok_or("original generated phase missing")?;
        let phase = history.span(index).unwrap();
        let segment_end = history
            .span(index + 1)
            .map(|p| {
                let w = p.to_words();
                TargetTime::new(w[4], w[5], w[6]).unwrap()
            })
            .unwrap_or(history.end_time());
        let source = self
            .output
            .converter_owner()
            .source_playback_history()
            .unwrap();
        let playback = (0..source.len())
            .filter_map(|i| source.span(i))
            .find(|span| {
                !phase
                    .compare_source_frame_at_interpolated_time(
                        q,
                        segment_end,
                        span.physical_start(),
                    )
                    .unwrap()
                    .is_lt()
                    && span.physical_end().is_none_or(|end| {
                        phase
                            .compare_source_frame_at_interpolated_time(q, segment_end, end)
                            .unwrap()
                            .is_lt()
                    })
            })
            .ok_or("actual source span missing")?;
        Ok(NativeTargetPlaybackQuery::new(
            7,
            self.basis,
            id,
            q,
            NativeTargetPlaybackFacts {
                playback: ConvertedPlaybackFacts {
                    phase,
                    segment_end,
                    generated_target_end: history.end_time(),
                    submitted_target_end: self.output.admitted_target_time().unwrap(),
                    playback,
                    pulled_source_end: self.output.mixer().frame_cursor(),
                    revision: 2 + 2 * self.frame,
                },
                source: self.output.last_real_source_report(),
                converted: Some(self.converted),
                boundaries: self.output.boundaries(),
                revision: 4 + 2 * self.frame,
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
        self.effects.set(self.effects.get() + 1);
        self.frame = 1 + self.step.get() as u64;
        while self.generated <= self.frame {
            self.converted = self.output.render_pending(8)?;
            self.output.admit(8)?;
            self.generated += 8;
        }
        let obs = self.observation();
        p.admit_target_coordinate(obs)?;
        let q = exact(
            self.basis
                .start_time()
                .checked_add_frames(self.frame, 1500)?,
        );
        let native = NativeTargetPlaybackObservation::new(obs, self.packet(q, id)?)?;
        Ok(Some(CoordinatePlaybackControlFacts::Target(
            TargetCoordinateControlFacts::from_target(&native, p, id)?,
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
        Ok(Some(NativePlaybackMapping::from_target(
            &self.packet(*q, id)?,
        )))
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(host(self.frame))
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(self.output.last_real_source_report())
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        self.effects.set(self.effects.get() + 1);
        // Each input comes from a genuine new acquired HOST prefix. The initial
        // Started commit sees no event; subsequent attempts receive real input.
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
        // With no pause or fixed NativeEnd owner the shared loop legitimately
        // acquires its sole native observation through this port. Preserve the
        // same genuine generated/admitted owner and original Target descriptor.
        self.effects.set(self.effects.get() + 1);
        self.frame = 1 + self.step.get() as u64;
        while self.generated <= self.frame {
            self.converted = self.output.render_pending(8)?;
            self.output.admit(8)?;
            self.generated += 8;
        }
        p.admit_target_coordinate(self.observation())?;
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
struct Control(Rc<Cell<usize>>, Rc<Cell<usize>>);
impl NativePumpControl for Control {
    type Moment = usize;
    fn now(&mut self) -> NativeGameplayResult<usize> {
        self.1.set(self.1.get() + 1);
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
    prepared: Vec<PlayerId>,
}
impl NativeGameplayHost for Host {
    fn prepare_play_policies(
        &mut self,
        policies: &[(PlayerId, &crate::play_policy::ResolvedPlayPolicy)],
    ) -> NativeGameplayResult<()> {
        crate::native_gameplay_host::NoopGameplayHost.prepare_play_policies(policies)?;
        self.prepared.extend(policies.iter().map(|(id, _)| *id));
        Ok(())
    }
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
    effects: Rc<Cell<usize>>,
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
        Self::new_with_end(ids, repeat, scrub, Timestamp::from_nanos(30_000_000))
    }
    fn new_with_end(ids: &[u32], repeat: bool, scrub: bool, end_song: Timestamp) -> Self {
        Self::new_with_rate(ids, repeat, scrub, end_song, Rate::new(3, 2).unwrap())
    }
    fn new_with_rate(
        ids: &[u32],
        repeat: bool,
        scrub: bool,
        end_song: Timestamp,
        rate: Rate,
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
        let program = PreparedPracticeProgram::new_with_speed(
            &bank,
            vec![],
            PracticeLimits::new(8, 1, 64, 8, 256).unwrap(),
            rate,
            Duration::ZERO,
            false,
        )
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
        let mut output = ConvertedNativeOutputState::new(
            mixer,
            DeviceFormat::new(1500, 1, SampleEncoding::Float32, None).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            8,
        )
        .unwrap_or_else(|_| panic!("valid converter"));
        output.enable_phase_history(128).unwrap();
        let basis = output.target_frame_basis();
        let converted = output.render_pending(8).unwrap();
        output.admit(8).unwrap();
        let phase = OutputPhaseBasis::new(ClockDomainId(3), 91, 1, 0, 1000).unwrap();
        let mut members = Vec::new();
        let mut configs = Vec::new();
        let mut states = Vec::new();
        let mut transport = None;
        for &id in ids {
            let launch = SessionLaunch::new(vec![
                "--chart".into(),
                "practice.bms".into(),
                "--speed".into(),
                format!("{}/{}", rate.numerator(), rate.denominator()),
                "--record-replay".into(),
                format!("player-{id}.bkr"),
            ])
            .unwrap();
            let attempt = prepare_attempt_with_speed(
                &original,
                &policy,
                &launch,
                PracticeAttemptConfig {
                    start: Timestamp::ZERO,
                    end: Some(end_song),
                    domain: ClockDomainId(3),
                    chart_seed: 0,
                    capture_limits: Some(limits()),
                },
                speed,
            )
            .unwrap();
            transport = Some(attempt.transport_at_output_phase(phase, at(3, 0)).unwrap());
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
                capture_limits: Some(limits()),
            });
        }
        let practice = PracticePlayback::new_with_speed(
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
        .unwrap();
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
        let effects = Rc::new(Cell::new(0));
        let device = Device {
            effects: effects.clone(),
            output,
            basis,
            converted,
            step: step.clone(),
            frame: 1,
            generated: 8,
            last_acquired_frame: None,
            close_at: 55,
            unavailable: false,
            scalar_calls: 0,
            queries: RefCell::new(vec![]),
            ids: ids.to_vec(),
        };
        let mut p = new_coordinate_audio_presentation(
            7,
            AudioAuthorityCoordinateBasis::TargetFrame(basis),
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
        let cold = CoordinateTargetNativeAudioSnapshot::new(
            TargetNativeAudioSnapshot {
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
        p.admit_target_coordinate(cold).unwrap();
        p.admit_target_coordinate(device.observation()).unwrap();
        p.pin_practice_mixer_basis(source_basis).unwrap();
        let stream = p.authority().epoch().stream_origin;
        Self {
            effects: effects.clone(),
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
                .with_target_basis(7, basis)
                .unwrap(),
            end: None,
            delivery: InputDeliveryTelemetry::new(128, ClockDomainId(1)).unwrap(),
            pre: 0,
            device,
            control: Control(step.clone(), effects),
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
                prepared: vec![],
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
        let ids: Vec<_> = self.states.iter().map(|state| state.player).collect();
        let policy =
            crate::play_policy::ResolvedPlayPolicy::builtin(10_000_000, 10_000_000, 0).unwrap();
        let policies: Vec<_> = ids.iter().map(|id| (*id, &policy)).collect();
        self.run_with_policies(&policies)
    }
    fn run_with_policies(
        &mut self,
        policies: &[(PlayerId, &crate::play_policy::ResolvedPlayPolicy)],
    ) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
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
        run_cohort_audio_coordinate_practice_with_policies_and_results_and_ports(
            &mut self.device,
            session,
            self.config,
            &mut self.control,
            &mut self.host,
            policies,
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
            Some(AudioAuthorityCoordinateBasis::TargetFrame(
                self.device.basis
            ))
        );
    }
}

fn hashes(f: &Fixture) -> Vec<u64> {
    f.states
        .iter()
        .map(|state| {
            f.group
                .member_judge(state.player)
                .unwrap()
                .stable_hash()
                .unwrap()
        })
        .collect()
}
fn assert_no_effects(f: &Fixture, before: &[u64]) {
    assert_eq!(
        f.effects.get(),
        0,
        "no device/control IO before cold admission succeeds"
    );
    assert!(f.host.prepared.is_empty());
    assert!(f.host.generations.is_empty());
    assert!(f.recordings.0.is_empty());
    assert!(f.host.identities.is_empty());
    assert!(f.host.commits.is_empty());
    assert_eq!(f.practice.generation(), 1);
    assert_eq!(f.group.transport().output_phase_basis(), Some(f.phase));
    assert_eq!(f.group.audio_scope(), CommandScope(1));
    assert!(f
        .states
        .iter()
        .all(|state| state.capture.as_ref().unwrap().records().is_empty()));
    assert_eq!(hashes(f), before);
}
fn replace_capture(
    f: &mut Fixture,
    member: usize,
    domain: ClockDomainId,
    speed: ReplaySpeed,
    policy: &crate::play_policy::ResolvedPlayPolicy,
) {
    let state = &mut f.states[member];
    let judge = f.group.member_judge(state.player).unwrap();
    state.capture = Some(
        LiveReplayCapture::new_with_policy_and_speed(
            judge,
            domain,
            limits(),
            Timestamp::ZERO,
            0,
            f.config.audio.gameplay.end_song,
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            policy,
            speed,
        )
        .unwrap(),
    );
}
#[test]
fn selected_three_halves_scrub_enters_real_cohort_ports_and_reconstructs_every_member() {
    for ids in [&[7, 29][..], &[7, 29, 101, 409][..]] {
        let mut f = Fixture::new(ids, false, true);
        f.device.close_at = 25;
        assert!(f.run().unwrap().is_none());
        f.assert_fresh();
        assert!(f.effects.get() > 0);
        assert_eq!(
            f.host.prepared.iter().map(|id| id.0).collect::<Vec<_>>(),
            ids
        );
        assert_eq!(
            f.recordings
                .0
                .iter()
                .map(|(id, _)| id.0)
                .collect::<Vec<_>>(),
            ids
        );
        assert!(f.host.commits.contains(&2));
    }
}
#[test]
fn selected_three_halves_loop_keeps_canonical_headers_and_original_roster() {
    let ids = [7, 29, 101, 409];
    let mut f = Fixture::new(&ids, true, false);
    assert!(f.run().unwrap().is_none());
    f.assert_fresh();
    assert!(f.recordings.0.len() >= ids.len());
    for captures in f.recordings.0.chunks(ids.len()) {
        assert_eq!(captures.iter().map(|(id, _)| id.0).collect::<Vec<_>>(), ids);
        for (_, capture) in captures {
            let setup =
                crate::replay_playback::decode_section_setup(&capture.header().options).unwrap();
            assert_eq!(setup.source_speed.rate(), Rate::new(3, 2).unwrap());
            assert_eq!(capture.header().normalized_clock, ClockDomainId(3));
        }
    }
}
#[test]
fn selected_practice_rejects_dirty_member_score_before_any_ports() {
    for member in 0..4 {
        let mut f = Fixture::new(&[7, 29, 101, 409], true, false);
        let before = hashes(&f);
        f.states[member].score.hits = 1;
        let error = f.run().unwrap_err();
        assert!(error.to_string().contains("pristine"), "{error}");
        assert_no_effects(&f, &before);
    }
}
#[test]
fn selected_practice_requires_exact_original_policy_ids_and_order_before_ports() {
    let policy =
        crate::play_policy::ResolvedPlayPolicy::builtin(10_000_000, 10_000_000, 0).unwrap();
    for ids in [
        &[7, 29, 101][..],
        &[29, 7, 101, 409][..],
        &[7, 29, 101, 410][..],
        &[7, 29, 101, 101][..],
    ] {
        let mut f = Fixture::new(&[7, 29, 101, 409], true, false);
        let before = hashes(&f);
        let policies: Vec<_> = ids.iter().map(|id| (PlayerId(*id), &policy)).collect();
        assert!(f.run_with_policies(&policies).is_err());
        assert_no_effects(&f, &before);
    }
}
#[test]
fn selected_practice_rejects_capture_speed_clock_and_selected_rules_before_ports() {
    let policy =
        crate::play_policy::ResolvedPlayPolicy::builtin(10_000_000, 10_000_000, 0).unwrap();
    for variant in 0..3 {
        let mut f = Fixture::new(&[7, 29], true, false);
        let before = hashes(&f);
        if variant == 0 {
            replace_capture(
                &mut f,
                1,
                ClockDomainId(3),
                ReplaySpeed::new(Rate::NORMAL).unwrap(),
                &policy,
            );
        } else if variant == 1 {
            let speed = ReplaySpeed::new(f.rate).unwrap();
            replace_capture(&mut f, 1, ClockDomainId(1), speed, &policy);
        }
        let wrong =
            crate::play_policy::ResolvedPlayPolicy::builtin(9_000_000, 10_000_000, 0).unwrap();
        let policies = [
            (PlayerId(7), &policy),
            (PlayerId(29), if variant == 2 { &wrong } else { &policy }),
        ];
        assert!(f.run_with_policies(&policies).is_err());
        assert_no_effects(&f, &before);
    }
}
#[test]
fn selected_practice_rejects_incoherent_section_and_source_grid_before_ports() {
    for variant in 0..3 {
        let mut f = Fixture::new(&[7, 29], true, false);
        let before = hashes(&f);
        match variant {
            0 => f.config.audio.section_start = Timestamp::from_nanos(1),
            1 => f.config.source_history = SourcePlaybackIdentity::new(2000, 0, 0).unwrap(),
            _ => f.config.audio.gameplay.end_song = Some(Timestamp::from_nanos(29_000_000)),
        }
        assert!(f.run().is_err());
        assert_no_effects(&f, &before);
    }
}

#[test]
fn selected_normal_coordinate_practice_keeps_existing_normal_header_meaning() {
    let mut f = Fixture::new_with_rate(
        &[7, 29],
        true,
        false,
        Timestamp::from_nanos(30_000_000),
        Rate::NORMAL,
    );
    assert!(f.run().unwrap().is_none());
    f.assert_fresh();
    assert_eq!(
        f.group.transport().anchors().last().unwrap().rate,
        Rate::NORMAL
    );
    assert_eq!(f.host.prepared, [PlayerId(7), PlayerId(29)]);
}

#[test]
fn selected_practice_refuses_uninstalled_scope_before_device_control_or_host_effects() {
    for scope in [CommandScope(0), CommandScope(2)] {
        let mut f = Fixture::new(&[7, 29], true, false);
        let before = hashes(&f);
        f.group.set_audio_scope(scope);
        assert!(f.run().is_err());
        assert_eq!(f.effects.get(), 0);
        assert!(f.host.prepared.is_empty());
        assert!(f.host.generations.is_empty());
        assert!(f.recordings.0.is_empty());
        assert_eq!(f.practice.generation(), 1);
        assert_eq!(f.group.audio_scope(), scope);
        assert_eq!(hashes(&f), before);
    }
}
