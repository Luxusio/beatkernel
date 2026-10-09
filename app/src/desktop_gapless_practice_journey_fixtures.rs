//! Actual desktop/thread/player/pump/factory/converter journey with injected IO.
//! Controlled native-port evidence is not evidence from physical hardware.
use super::*;
use beatkernel::{
    audio::*,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeStage},
    replay::codec::ReplayCodecLimits,
    runtime::{RuntimeProcessingClock, SoundBinding},
    telemetry::InputDeliveryTelemetry,
    time::{ClockDomainId, ClockPair, ClockPoint, Duration as SongDuration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms_runtime::{
    audio_authority::AudioAuthorityConfig,
    competition::ScoreSummary,
    gameplay::output::{
        application::owner::GameplayOutputOwner,
        ports::{OriginalTargetNativeOutputBackend, OutputReplacementBackend},
    },
    local_input::InputMerger,
    local_runtime::SoloRuntime,
    native_audio::{prepare_retained_audio, NativeAudioConfig, NativePracticeAudioConfig},
    native_audio_presentation::{NativeAudioPresentation, TargetNativeAudioSnapshot},
    native_gameplay::{
        run_gameplay_audio_with_practice_and_result_and_score, AudioGameplayConfig,
        AudioGameplaySession, GameplaySession, InputBatch, NativeGameplayConfig,
        NativeGameplayDevice, NativeGameplayResult,
    },
    native_judge::NativeJudgeConfig,
    play_policy::{GaugeSelection, OriginalGaugeContext},
    playback_pause::NativePause,
    practice_playback::{PracticeMember, PracticePlayback, PracticeRecordingPort},
    replay_capture::LiveReplayCapture,
};
use beatkernel_platform::audio::presentation::{
    discipline::PresentationDiscipline, validation::OriginalNativePresentationEvidence,
};
use beatkernel_platform::audio::{
    ConvertedBoundaryFacts, ConvertedNativeOutputState, DeviceFormat, SampleEncoding,
};
use std::{
    collections::VecDeque,
    sync::{mpsc, Mutex},
};

const WAIT: Duration = Duration::from_secs(5);
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
#[derive(Default)]
struct Trace {
    opens: usize,
    retires: usize,
    pcm: Vec<f32>,
    owner_address: Option<usize>,
    storage_address: Option<usize>,
    partials: usize,
    paths: Vec<PathBuf>,
    captures: Vec<LiveReplayCapture>,
    acquired: Vec<PhysicalInputEvent>,
    final_generation: u64,
    final_epoch: u64,
}
struct Endpoint {
    owner: Option<Box<ConvertedNativeOutputState>>,
    basis: TargetFrameBasis,
    epoch: u64,
    presented: u64,
    retired: bool,
    held: bool,
}
impl StoppedMixerSource<ConvertedNativeOutputState> for Endpoint {
    type Error = io::Error;
    fn take_stopped_mixer(&mut self) -> Result<Option<ConvertedNativeOutputState>, io::Error> {
        if !self.retired {
            return Err(io::Error::other("live endpoint cannot release owner"));
        }
        Ok(self.owner.take().map(|owner| *owner))
    }
}
struct Factory {
    trace: Arc<Mutex<Trace>>,
}
impl OutputReplacementBackend<ConvertedNativeOutputState, TargetFrameBasis> for Factory {
    type Presentation = PresentationDiscipline;
    type Output = Endpoint;
    type Request = ();
    type Error = io::Error;
    fn open(
        &mut self,
        _: (),
        owner: ConvertedNativeOutputState,
        epoch: u64,
    ) -> Result<Endpoint, OutputOpenFailure<io::Error, Endpoint, ConvertedNativeOutputState>> {
        let mut trace = self.trace.lock().unwrap();
        trace.opens += 1;
        if trace.opens != 1 {
            return Err(OutputOpenFailure::recovered_state(
                io::Error::other("second native factory open forbidden"),
                Some(owner),
            ));
        }
        let basis = owner.target_frame_basis();
        let owner = Box::new(owner);
        trace.owner_address = Some((&*owner as *const ConvertedNativeOutputState) as usize);
        Ok(Endpoint {
            owner: Some(owner),
            basis,
            epoch,
            presented: 0,
            retired: false,
            held: false,
        })
    }
    fn retire(&mut self, output: &mut Endpoint) -> io::Result<()> {
        output.retired = true;
        self.trace.lock().unwrap().retires += 1;
        Ok(())
    }
    fn start(&mut self, _: &mut Endpoint) -> io::Result<()> {
        Ok(())
    }
    fn epoch(&self, output: &Endpoint) -> u64 {
        output.epoch
    }
    fn basis(&self, output: &Endpoint) -> TargetFrameBasis {
        output.basis
    }
    fn observe(&mut self, _: &mut Endpoint, _: &mut PresentationDiscipline) -> io::Result<()> {
        Err(io::Error::other("legacy observer forbidden"))
    }
    fn render_report(&self, output: &Endpoint) -> io::Result<Option<RenderReport>> {
        Ok(output.owner.as_ref().unwrap().last_real_source_report())
    }
}
impl OriginalTargetNativeOutputBackend<ConvertedNativeOutputState> for Factory {
    fn planned_target_basis(
        &self,
        _: &(),
        owner: &ConvertedNativeOutputState,
    ) -> io::Result<TargetFrameBasis> {
        Ok(owner.target_frame_basis())
    }
    fn observe_native_target(
        &mut self,
        output: &mut Endpoint,
    ) -> io::Result<Option<TargetNativeAudioSnapshot>> {
        let owner = output.owner.as_mut().unwrap();
        let mut trace = self.trace.lock().unwrap();
        assert_eq!(
            trace.owner_address,
            Some((&**owner as *const ConvertedNativeOutputState) as usize)
        );
        let count = if owner.pending_frames() == 0 {
            if output.held {
                owner.render_held_pending(6)
            } else {
                owner.render_pending(6)
            }
            .map_err(io::Error::other)?;
            let storage = owner.pending_samples().as_ptr() as usize;
            match trace.storage_address {
                Some(previous) => assert_eq!(previous, storage),
                None => trace.storage_address = Some(storage),
            }
            trace.partials += 1;
            2 // Keep four genuine generated target samples unread.
        } else {
            owner.pending_frames()
        };
        trace
            .pcm
            .extend_from_slice(&owner.pending_samples()[..count]);
        owner.admit(count).map_err(io::Error::other)?;
        output.presented += count as u64;
        let raw = output
            .basis
            .point_at_stream_frame(output.presented)
            .map_err(io::Error::other)?;
        Ok(Some(TargetNativeAudioSnapshot {
            epoch: output.epoch,
            basis: output.basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: raw,
                target: point(1, raw.timestamp.as_nanos()),
            }),
        }))
    }
    fn converted_report(&self, output: &Endpoint) -> io::Result<Option<ConvertedRenderReport>> {
        Ok(output.owner.as_ref().unwrap().pending_report())
    }
    fn boundary_facts(&self, output: &Endpoint) -> io::Result<ConvertedBoundaryFacts> {
        Ok(output.owner.as_ref().unwrap().boundaries())
    }
    fn set_held(&mut self, output: &mut Endpoint, held: bool) -> io::Result<()> {
        output.held = held;
        Ok(())
    }
}
type Owned = GameplayOutputOwner<Factory, ConvertedNativeOutputState, TargetFrameBasis>;
struct Device {
    output: Owned,
    trace: Arc<Mutex<Trace>>,
    proceed: mpsc::Receiver<()>,
    steps: mpsc::SyncSender<u64>,
    sequence: u64,
}
impl NativeGameplayDevice for Device {
    fn audio_pause_observation(
        &mut self,
        presentation: &NativeAudioPresentation,
        now: ClockPoint,
    ) -> NativeGameplayResult<beatkernel_bms_runtime::live_pause::LivePauseObservation> {
        self.output
            .audio_pause_observation_target(presentation, now)
            .map_err(|error| error.to_string().into())
    }
    fn set_audio_held(&mut self, held: bool) -> NativeGameplayResult<()> {
        self.output
            .set_target_held(held)
            .map_err(|error| error.to_string().into())
    }

    fn observe_audio(
        &mut self,
        presentation: &mut NativeAudioPresentation,
    ) -> NativeGameplayResult<()> {
        self.steps.send(self.output.current().unwrap().presented)?;
        self.proceed.recv_timeout(WAIT)?;
        if !player::cancelled() {
            self.output
                .observe_target_native(presentation)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
    fn observe(&mut self, _: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
        Err("legacy observation forbidden".into())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(self.output.render_report())
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        let endpoint = self.output.current().unwrap();
        let raw = endpoint.basis.point_at_stream_frame(endpoint.presented)?;
        Ok(point(1, raw.timestamp.as_nanos()))
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        let now = self.host_now()?;
        for (at, state) in [
            (8_000_000, ButtonState::Down),
            (12_000_000, ButtonState::Up),
        ] {
            let expected = if state == ButtonState::Down { 0 } else { 1 };
            if self.sequence == expected && now.timestamp.as_nanos() >= at {
                let event = PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(1), point(1, at), self.sequence),
                    control: PhysicalControlId::keyboard(7),
                    state,
                });
                events.push_back(event.clone());
                self.trace.lock().unwrap().acquired.push(event);
                self.sequence += 1;
            }
        }
        Ok(InputBatch {
            completed_through: Some(now),
            backlog: false,
            closed: false,
        })
    }
    fn observe_end(
        &mut self,
        _: &mut beatkernel_bms_runtime::native_end::NativeEnd,
        _: &PresentationDiscipline,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<beatkernel_bms_runtime::native_end::EndBoundary>> {
        Err("legacy end forbidden".into())
    }
    fn seed_resume(
        &mut self,
        _: &mut PresentationDiscipline,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        Err("legacy resume forbidden".into())
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        Err("host fallback forbidden".into())
    }
}
struct Recordings(Arc<Mutex<Trace>>);
impl PracticeRecordingPort for Recordings {
    fn archive(
        &mut self,
        member: beatkernel_bms_runtime::local_players::PlayerId,
        path: &std::path::Path,
        capture: LiveReplayCapture,
    ) -> NativeGameplayResult<()> {
        assert_eq!(member, beatkernel_bms_runtime::local_players::PlayerId(1));
        let mut trace = self.0.lock().unwrap();
        assert!(!trace.paths.iter().any(|previous| previous == path));
        trace.paths.push(path.to_owned());
        trace.captures.push(capture);
        Ok(())
    }
}

#[test]
fn desktop_gapless_practice_actual_worker_pump_retains_factory_converter_across_loop_disable_scrub()
{
    let trace = Arc::new(Mutex::new(Trace::default()));
    let owner_trace = Arc::clone(&trace);
    let (proceed, owner_proceed) = mpsc::sync_channel(1);
    let (owner_steps, steps) = mpsc::sync_channel(1);
    let (done_tx, done) = mpsc::sync_channel(1);
    let launch = SessionLaunch::new(vec![
        "--chart".into(),
        "connected.bms".into(),
        "--record-replay".into(),
        "connected.bkr".into(),
    ])
    .unwrap();
    let game = spawn_game_with(
        move |args| {
            let result = (|| -> NativeGameplayResult<()> {
                let launch = player::native_launch(args)?;
                let source = beatkernel_bms::parse(
                    "#BPM 7500\n#WAV01 key.wav\n#WAV02 bgm.wav\n#00001:02\n#00011:00010000\n",
                    Default::default(),
                )?;
                let compiled = source.compile()?;
                assert_eq!(
                    compiled.chart.objects()[0].time.start,
                    Timestamp::from_nanos(8_000_000)
                );
                let domain = ClockDomainId(3);
                let policy = NativeJudgeConfig {
                    early: 0,
                    late: 0,
                    offset: 0,
                    preroll: 0,
                    output: domain,
                    end: Some(Timestamp::from_nanos(200_000_000)),
                }
                .resolve_play_policy(
                    &OriginalGaugeContext::from_source(&source),
                    GaugeSelection::BeatKernel,
                )?;
                player::publish_chart(&source, &compiled.chart)?;
                let format = AudioFormat::new(1000, 1)?;
                let pcm = PcmLimits::new(1024, 4096, 2)?;
                let mut bank = SampleBank::new(format, pcm)?;
                bank.insert(SampleId(1), PcmSample::new(format, vec![0.; 8], pcm)?)?;
                bank.insert(SampleId(2), PcmSample::new(format, vec![0.25; 256], pcm)?)?;
                let region = PracticeRegion::new(
                    Timestamp::ZERO,
                    Timestamp::from_nanos(200_000_000),
                    false,
                )?;
                let prepared = prepare_retained_audio(
                    bank,
                    vec![AudioCommand::Play {
                        voice: VoiceId(800),
                        sample: SampleId(2),
                        at: Timestamp::ZERO,
                        gain: 1.,
                    }],
                    NativeAudioConfig {
                        output_origin: point(2, 0),
                        start: Timestamp::ZERO,
                        preroll: SongDuration::ZERO,
                        lookahead: SongDuration::from_nanos(100_000_000),
                        voices: 4,
                        max_render_frames: 64,
                        playback_end_frame: None,
                        gated_start: false,
                    },
                    NativePracticeAudioConfig {
                        region,
                        limits: PracticeLimits::new(4, 1, 256, 8, 64)?,
                    },
                )?;
                let mixer_basis = prepared.audio.mixer.output_frame_basis();
                let owner = ConvertedNativeOutputState::new(
                    prepared.audio.mixer,
                    DeviceFormat::new(1500, 1, SampleEncoding::Float32, None)?,
                    ChannelMatrix::default_mix(1, 1)?,
                    ResampleQuality::Linear,
                    6,
                )
                .map_err(|failure| failure.into_parts().0)?;
                let output = Owned::open_initial(
                    Factory {
                        trace: Arc::clone(&owner_trace),
                    },
                    (),
                    owner,
                    0,
                )
                .map_err(|failure| failure.into_parts().0)?;
                let basis = output.current().unwrap().basis;
                let mut device = Device {
                    output,
                    trace: Arc::clone(&owner_trace),
                    proceed: owner_proceed,
                    steps: owner_steps,
                    sequence: 0,
                };
                let limits =
                    ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024)?)?;
                let judge = JudgeEngine::new(
                    compiled.chart.clone(),
                    source.rules(),
                    policy.judge().clone(),
                )?;
                let mut capture = Some(LiveReplayCapture::new_with_policy(
                    &judge,
                    domain,
                    limits,
                    Timestamp::ZERO,
                    0,
                    Some(region.end),
                    beatkernel_bms::BmsInputMode::ButtonOnly,
                    None,
                    &policy,
                )?);
                let sounds = compiled
                    .chart
                    .objects()
                    .iter()
                    .map(|object| SoundBinding {
                        object: object.id,
                        stage: JudgeStage::Instant,
                        sample: SampleId(1),
                        voice: VoiceId(object.id.0),
                        gain: 1.,
                    })
                    .collect();
                let mut runtime = SoloRuntime::new(
                    domain,
                    ClockDomainId(2),
                    Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
                    BindingMap::from_bindings([Binding {
                        device: DeviceSelector::Any,
                        physical: PhysicalControlId::keyboard(7),
                        game_control: GameControlId(0x11),
                    }])?,
                    judge,
                    prepared.audio.producer,
                    sounds,
                    16,
                )?;
                runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
                runtime.set_audio_scope(CommandScope(1));
                runtime.set_song_end(region.end)?;
                let mut bgm = prepared.audio.bgm;
                let mut gauge =
                    beatkernel_bms_runtime::gauge::BmsGauge::new(policy.gauge().try_copy()?);
                let mut presentation =
                    beatkernel_bms_runtime::native_audio_startup::new_target_audio_presentation(
                        0,
                        basis,
                        ClockDomainId(1),
                        point(3, 0),
                        AudioAuthorityConfig::default(),
                    )?;
                presentation.admit_target(TargetNativeAudioSnapshot {
                    epoch: 0,
                    basis,
                    evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                        source: point(2, 0),
                        target: point(1, 0),
                    }),
                })?;
                let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 1000)?
                    .with_target_basis(0, basis)?;
                let mut end = None;
                let mut completion = None;
                let mut competition = None;
                let mut delivery = InputDeliveryTelemetry::new(16, ClockDomainId(1))?;
                let mut pre = 0;
                let mut merger = InputMerger::new_dynamic(ClockDomainId(1), point(1, 0), 4096, 16)?;
                let member_policy = NativeJudgeConfig {
                    early: 0,
                    late: 0,
                    offset: 0,
                    preroll: 0,
                    output: domain,
                    end: Some(region.end),
                }
                .resolve_play_policy(
                    &OriginalGaugeContext::from_source(&source),
                    GaugeSelection::BeatKernel,
                )?;
                let mut practice = PracticePlayback::new(
                    prepared.practice,
                    source,
                    vec![PracticeMember {
                        player: beatkernel_bms_runtime::local_players::PlayerId(1),
                        policy: member_policy,
                        launch,
                        chart_seed: 0,
                        capture_limits: Some(limits),
                    }],
                    region,
                    region.end,
                    mixer_basis,
                    2,
                )?;
                let mut score = ScoreSummary::default();
                let outcome = run_gameplay_audio_with_practice_and_result_and_score(
                    &mut device,
                    AudioGameplaySession {
                        session: GameplaySession {
                            runtime: &mut runtime,
                            gauge: &mut gauge,
                            bgm: &mut bgm,
                            discipline: &mut presentation,
                            pause: &mut pause,
                            end: &mut end,
                            completion: &mut completion,
                            capture: &mut capture,
                            competition: &mut competition,
                            delivery: &mut delivery,
                            pre_origin_inputs: &mut pre,
                        },
                        merger: &mut merger,
                    },
                    AudioGameplayConfig {
                        gameplay: NativeGameplayConfig {
                            origin: point(1, 0),
                            stream_origin: point(2, 0),
                            playback_origin: point(2, 0),
                            song_origin: Timestamp::ZERO,
                            sample_rate: 1000,
                            end_song: Some(region.end),
                            advance_lag: SongDuration::ZERO,
                            seconds: None,
                            pause_supported: true,
                            logical_schedule: true,
                        },
                        section_start: Timestamp::ZERO,
                    },
                    &mut score,
                    &policy,
                    &mut practice,
                    &mut Recordings(Arc::clone(&owner_trace)),
                );
                let mut trace = owner_trace.lock().unwrap();
                trace.final_generation = practice.generation();
                trace.final_epoch = presentation.authority().epoch().id;
                drop(trace);
                let path = practice.members()[0]
                    .launch
                    .args()
                    .windows(2)
                    .find(|pair| pair[0] == "--record-replay")
                    .map(|pair| std::path::PathBuf::from(&pair[1]))
                    .expect("recorded current attempt path");
                assert!(outcome.as_ref().unwrap().is_none());
                assert!(
                    capture.is_some(),
                    "cancelled worker retains final live capture"
                );
                let stopped = device
                    .output
                    .stop()
                    .map_err(|error| -> Box<dyn std::error::Error> { error.into() });
                beatkernel_bms_runtime::native_finish::finish_solo_with_result_and_score(
                    outcome,
                    stopped,
                    Ok(()),
                    None,
                    capture.take(),
                    gauge.profile(),
                    &score,
                    Some(&path),
                    |capture, actual, failed| {
                        assert!(!failed);
                        assert_eq!(actual, Some(path.as_path()));
                        let mut trace = owner_trace.lock().unwrap();
                        assert!(!trace.paths.iter().any(|old| old == &path));
                        trace.paths.push(path.clone());
                        trace.captures.push(capture.unwrap());
                        Ok(())
                    },
                    |_, _| panic!("cancelled worker must not publish a completed sidecar"),
                )?;
                Ok(())
            })()
            .map_err(|error| error.to_string());
            let _ = done_tx.send(result.clone());
            result
        },
        launch.clone(),
        0,
        false,
        false,
    )
    .unwrap();
    let mut app = tests::lifecycle_fixture();
    let next = app
        .prepare_route(ScreenRoute::Play { replay: false })
        .unwrap();
    app.commit_route(next);
    app.game = Some(game);
    let initial_step = steps.recv_timeout(WAIT).unwrap_or_else(|error| {
        panic!(
            "worker bootstrap: {error}; result: {:?}",
            done.recv_timeout(WAIT)
        )
    });
    assert_eq!(initial_step, 0);
    let tick = |app: &mut Desktop| {
        // Production publication coalesces frames by actual wall time. This
        // cold test wait permits delivery; it never supplies gameplay time.
        thread::sleep(Duration::from_millis(20));
        proceed.send(()).unwrap();
        let frame = steps.recv_timeout(WAIT).unwrap_or_else(|error| {
            panic!(
                "worker step: {error}; result: {:?}",
                done.recv_timeout(WAIT)
            )
        });
        app.collect_game();
        frame
    };
    for _ in 0..3 {
        tick(&mut app);
    }
    assert_eq!(
        app.game.as_ref().unwrap().snapshot.as_ref().unwrap().status,
        player::PlayerStatus::Playing
    );
    app.key(KeyCode::F7, false);
    let start = app.game.as_ref().unwrap().practice_bookmark.unwrap();
    for _ in 0..8 {
        tick(&mut app);
    }
    app.key(KeyCode::F10, false);
    let region = app.game.as_ref().unwrap().practice_loop.unwrap();
    assert!(region.end().nanoseconds() > start.nanoseconds());
    app.key(KeyCode::F11, false);
    let initial_id = app
        .game
        .as_ref()
        .unwrap()
        .viewer
        .pending_practice_request()
        .unwrap()
        .unwrap()
        .id;
    for _ in 0..12 {
        tick(&mut app);
        if app.game.as_ref().unwrap().loop_enabled {
            break;
        }
    }
    assert!(app.game.as_ref().unwrap().loop_enabled);
    assert!(!app.game.as_ref().unwrap().cancelling);
    let first_attempt = app.game.as_ref().unwrap().launch.attempt();
    for _ in 0..40 {
        tick(&mut app);
        if app.game.as_ref().unwrap().launch.attempt() >= first_attempt + 2 {
            break;
        }
    }
    assert!(app.game.as_ref().unwrap().launch.attempt() >= first_attempt + 2);
    app.key(KeyCode::F11, false);
    let disable_id = app
        .game
        .as_ref()
        .unwrap()
        .viewer
        .pending_practice_request()
        .unwrap()
        .unwrap()
        .id;
    assert!(disable_id > initial_id);
    for _ in 0..12 {
        tick(&mut app);
        if !app.game.as_ref().unwrap().viewer.practice_pending() {
            break;
        }
    }
    assert!(!app.game.as_ref().unwrap().loop_enabled);
    app.key(KeyCode::F8, false);
    let scrub_id = app
        .game
        .as_ref()
        .unwrap()
        .viewer
        .pending_practice_request()
        .unwrap()
        .unwrap()
        .id;
    assert!(scrub_id > disable_id);
    for _ in 0..12 {
        tick(&mut app);
        if !app.game.as_ref().unwrap().viewer.practice_pending() {
            break;
        }
    }
    assert!(!app.game.as_ref().unwrap().viewer.practice_pending());
    assert!(app.failure.is_none(), "{:?}", app.failure);
    assert_eq!(trace.lock().unwrap().opens, 1);
    let attempt = app.game.as_ref().unwrap().launch.attempt();
    app.key(KeyCode::F5, false);
    assert_eq!(
        app.game
            .as_ref()
            .unwrap()
            .prepared_retry
            .as_ref()
            .unwrap()
            .attempt(),
        attempt + 1
    );
    app.occluded = true;
    proceed.send(()).unwrap();
    assert_eq!(done.recv_timeout(WAIT).unwrap(), Ok(()));
    let deadline = Instant::now() + WAIT;
    while !app
        .game
        .as_ref()
        .unwrap()
        .worker
        .as_ref()
        .unwrap()
        .is_finished()
    {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    app.collect_game();
    assert!(app.game.as_ref().unwrap().joined);
    let trace = trace.lock().unwrap();
    assert_eq!((trace.opens, trace.retires, trace.final_epoch), (1, 1, 0));
    assert!(trace.partials > 8);
    assert!(trace.pcm.len() > 48);
    assert_eq!(&trace.pcm[..12], &[0.25; 12]);
    assert!(trace.pcm.iter().all(|sample| *sample == 0.25));
    assert_eq!(trace.acquired.len(), 2);
    assert_eq!(trace.paths[0], PathBuf::from("connected.bkr"));
    for (ordinal, path) in trace.paths.iter().enumerate().skip(1) {
        assert_eq!(
            path,
            &PathBuf::from(format!("connected.retry{ordinal}.bkr"))
        );
    }
    assert_eq!(trace.paths.len(), trace.captures.len());
    assert!(trace.captures.len() >= 4);
    assert!(trace
        .captures
        .iter()
        .all(|capture| capture.header().normalized_clock == ClockDomainId(3)));
    let first_input = trace.captures[0]
        .records()
        .iter()
        .find_map(|record| {
            if let beatkernel::replay::ReplayOperation::Input(input) = &record.operation {
                (input.physical.meta().sequence == 0).then_some((record.song_time, input))
            } else {
                None
            }
        })
        .expect("original acquired input must survive the archived attempt");
    assert_eq!(first_input.0, Timestamp::from_nanos(8_000_000));
    assert_eq!(first_input.1.physical.meta().source, DeviceId(1));
    assert_eq!(first_input.1.physical.meta().clock_domain, ClockDomainId(3));
    assert_eq!(
        first_input.1.physical.meta().original_clock_point,
        Some(point(1, 8_000_000))
    );
    let codec_limits =
        ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    for capture in &trace.captures {
        let file = beatkernel::replay::codec::ReplayFile::new(
            capture.header().clone(),
            capture.records().to_vec(),
        );
        let bytes = beatkernel::replay::codec::encode_replay(&file, codec_limits).unwrap();
        let decoded = beatkernel::replay::codec::decode_replay(&bytes, codec_limits).unwrap();
        assert_eq!(decoded.header, file.header);
        assert_eq!(decoded.records, file.records);
    }
}
