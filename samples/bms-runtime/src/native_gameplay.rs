//! One solo gameplay owner; native adapters acquire evidence and own cleanup.
use crate::{
    bgm::BgmFeeder,
    competition_live::LiveCompetition,
    completion::SongCompletion,
    local_runtime::SoloRuntime,
    native_end::{EndBoundary, NativeEnd},
    playback_pause::{NativePause, PauseKeyboard, PausePhase},
    player::{self, PauseState},
    replay_capture::LiveReplayCapture,
};
use beatkernel::{
    audio::RenderReport,
    input::PhysicalInputEvent,
    runtime::RuntimeReport,
    telemetry::InputDeliveryTelemetry,
    time::{
        ClockDomainId, ClockMapper, ClockMappingQuality, ClockPair, ClockPoint, Duration, Timestamp,
    },
};
use beatkernel_platform::audio::presentation::discipline::{
    DisciplineConfig, DisciplineUpdate, PresentationDiscipline,
};
use std::{
    collections::VecDeque,
    time::{Duration as WallDuration, Instant},
};

pub type NativeGameplayResult<T> = Result<T, Box<dyn std::error::Error>>;
pub const MAX_PENDING_INPUT_EVENTS: usize = 65_536;

/// Retains the original event atomically on bounded admission failure.
pub fn retain_input(
    events: &mut VecDeque<PhysicalInputEvent>,
    event: PhysicalInputEvent,
) -> NativeGameplayResult<()> {
    if events.len() >= MAX_PENDING_INPUT_EVENTS {
        return Err("native pending input capacity exceeded; restart required".into());
    }
    events.try_reserve(1)?;
    events.push_back(event);
    Ok(())
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputBatch {
    pub backlog: bool,
    pub closed: bool,
}

pub trait NativeGameplayDevice {
    fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()>;
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>>;
    fn host_now(&self) -> NativeGameplayResult<ClockPoint>;
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch>;
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        discipline: &PresentationDiscipline,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>>;
    /// Reseed using the original native observation source, never a fabricated snapshot.
    fn seed_resume(
        &mut self,
        discipline: &mut PresentationDiscipline,
        reference: ClockPair,
    ) -> NativeGameplayResult<()>;
    fn fallback_schedule(&mut self, rate: u32) -> NativeGameplayResult<ClockPoint>;
}
#[derive(Clone, Copy, Debug)]
pub struct NativeGameplayConfig {
    pub origin: ClockPoint,
    pub stream_origin: ClockPoint,
    pub playback_origin: ClockPoint,
    pub song_origin: Timestamp,
    pub sample_rate: u32,
    pub end_song: Option<Timestamp>,
    pub advance_lag: Duration,
    pub seconds: Option<u64>,
    pub pause_supported: bool,
    pub logical_schedule: bool,
}
pub struct NativeGameplaySession<'a> {
    pub runtime: &'a mut SoloRuntime,
    pub bgm: &'a mut BgmFeeder,
    pub discipline: &'a mut PresentationDiscipline,
    pub pause: &'a mut NativePause,
    pub end: &'a mut Option<NativeEnd>,
    pub completion: &'a mut Option<SongCompletion>,
    pub capture: &'a mut Option<LiveReplayCapture>,
    pub competition: &'a mut Option<LiveCompetition>,
    pub delivery: &'a mut InputDeliveryTelemetry,
    pub pre_origin_inputs: &'a mut u64,
}
struct ExplicitDomains;
impl ClockMapper for ExplicitDomains {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn point(event: &PhysicalInputEvent) -> ClockPoint {
    ClockPoint {
        domain: event.meta().clock_domain,
        timestamp: event.meta().timestamp,
    }
}
fn chronology(at: ClockPoint, last: ClockPoint) -> NativeGameplayResult<()> {
    if at.domain != last.domain || at.timestamp < last.timestamp {
        return Err("native input/operation host chronology regressed; restart required".into());
    }
    Ok(())
}
fn watermark(
    origin: ClockPoint,
    last: ClockPoint,
    now: ClockPoint,
    lag: Duration,
    backlog: bool,
) -> NativeGameplayResult<Option<ClockPoint>> {
    if origin.domain != now.domain
        || last.domain != now.domain
        || !(0..=1_000_000_000).contains(&lag.as_nanos())
    {
        return Err("invalid native deadline watermark domain/lag".into());
    }
    if backlog || now.timestamp < origin.timestamp {
        return Ok(None);
    }
    let at = (i128::from(now.timestamp.as_nanos()) - i128::from(lag.as_nanos()))
        .max(i128::from(origin.timestamp.as_nanos()))
        .max(i128::from(last.timestamp.as_nanos()));
    Ok(Some(ClockPoint {
        domain: now.domain,
        timestamp: Timestamp::from_nanos(i64::try_from(at)?),
    }))
}
fn schedule<D: NativeGameplayDevice>(
    device: &mut D,
    session: &NativeGameplaySession<'_>,
    config: NativeGameplayConfig,
) -> NativeGameplayResult<ClockPoint> {
    if config.logical_schedule {
        Ok(session.pause.scheduling_point(
            device
                .render_report()?
                .or_else(|| session.pause.last_render_report())
                .ok_or("mixer playback boundary unavailable for scheduling")?,
        )?)
    } else {
        device.fallback_schedule(config.sample_rate)
    }
}
fn publish(
    session: &mut NativeGameplaySession<'_>,
    report: RuntimeReport,
) -> NativeGameplayResult<()> {
    if let Some(capture) = session.capture.as_mut() {
        capture.record_report(&report)?;
    }
    player::publish_report(&report)?;
    if let Some(competition) = session.competition.as_mut() {
        competition.observe(&report)?;
    }
    for result in &report.judge_events {
        println!("judge={result:?}");
    }
    if !report.audio_failures.is_empty() {
        eprintln!("exact failed audio commands={:?}", report.audio_failures);
    }
    if let Some(error) = report.judge_error {
        return Err(error.into());
    }
    Ok(())
}
fn process<D: NativeGameplayDevice>(
    device: &mut D,
    session: &mut NativeGameplaySession<'_>,
    config: NativeGameplayConfig,
    event: PhysicalInputEvent,
) -> NativeGameplayResult<Timestamp> {
    let at = schedule(device, session, config)?;
    let report = session.runtime.process_input(event, &ExplicitDomains, at)?;
    let song = report.song_time;
    publish(session, report)?;
    Ok(song)
}
fn finite_done(
    config: NativeGameplayConfig,
    boundary: Option<EndBoundary>,
    last: ClockPoint,
    song: Timestamp,
    backlog: bool,
    resuming: bool,
) -> bool {
    config.end_song.is_some_and(|end| song >= end)
        && boundary.is_some_and(|boundary| {
            boundary.host.domain == last.domain && last.timestamp >= boundary.host.timestamp
        })
        && !backlog
        && !resuming
}

/// Complete solo pump. Device cancellation and all errors leave native cleanup to caller.
/// No native operation, wall-time inference or platform branch enters judge/audio callbacks.
pub fn run_gameplay<D: NativeGameplayDevice>(
    device: &mut D,
    mut session: NativeGameplaySession<'_>,
    config: NativeGameplayConfig,
) -> NativeGameplayResult<()> {
    if config.origin.domain == config.stream_origin.domain
        || config.stream_origin.domain != config.playback_origin.domain
        || config.playback_origin.timestamp < config.stream_origin.timestamp
        || config.sample_rate == 0
        || config.sample_rate > 1_000_000_000
        || !(0..=1_000_000_000).contains(&config.advance_lag.as_nanos())
    {
        return Err("invalid native gameplay clocks/rate/lag".into());
    }
    let deadline = config
        .seconds
        .map(|seconds| {
            Instant::now()
                .checked_add(WallDuration::from_secs(seconds))
                .ok_or("native gameplay deadline overflow")
        })
        .transpose()?;
    let mut pending = VecDeque::new();
    pending.try_reserve_exact(4096)?;
    let mut last_acquired = config.origin;
    let mut last_operation = config.origin;
    let mut last_song = config.song_origin;
    let mut last_progress = None;
    let mut last_host = None;
    let mut keyboard = PauseKeyboard::new();
    let mut paused_boundary: Option<ClockPoint> = None;
    let mut resume_boundary: Option<ClockPoint> = None;
    let mut pause_committed = false;
    let mut end_boundary = None;
    let mut end_rendered = false;
    if config.pause_supported {
        player::publish_pause(PauseState::Running);
    }
    while !player::cancelled() && deadline.is_none_or(|deadline| Instant::now() < deadline) {
        player::retry_pause_publication();
        device.observe(session.discipline)?;
        let reference = session
            .discipline
            .latest_pair()
            .ok_or("gameplay requires native clock relation")?;
        let rendered = device.render_report()?;
        if let Some(end) = session.end.as_mut() {
            end_rendered |=
                rendered.is_some_and(|report| report.playback_end_physical_frame.is_some());
            if let Some(boundary) = device.observe_end(end, session.discipline, rendered)? {
                end_boundary = Some(boundary);
            }
        }
        if config.pause_supported
            && !end_rendered
            && (session.pause.phase() == PausePhase::Running || pause_committed)
            && resume_boundary.is_none()
            && session
                .pause
                .request(player::pause_requested(), reference)?
        {
            let desired = session.pause.phase() == PausePhase::Pausing;
            session.runtime.request_audio_pause(desired);
            player::publish_pause(if desired {
                PauseState::Pausing
            } else {
                PauseState::Resuming
            });
        }
        if config.logical_schedule || config.pause_supported {
            if let Some(boundary) = session.pause.observe(rendered, reference)? {
                if boundary.paused {
                    if !end_rendered {
                        session
                            .runtime
                            .transport_mut()
                            .pause(boundary.host.timestamp)?;
                        paused_boundary = Some(boundary.host);
                        pause_committed = false;
                    }
                } else {
                    session
                        .runtime
                        .transport_mut()
                        .resume(boundary.host.timestamp)?;
                    resume_boundary = Some(boundary.host);
                    paused_boundary = None;
                    pause_committed = false;
                    let mut discipline = PresentationDiscipline::new_with_playback_origin(
                        DisciplineConfig::default(),
                        config.stream_origin,
                        config.playback_origin,
                        config.origin.domain,
                        session.pause.song_origin_after_pause(config.song_origin)?,
                    )?;
                    device.seed_resume(&mut discipline, reference)?;
                    *session.discipline = discipline;
                }
            }
        }
        if let Some(report) = device.render_report()?.filter(|report| !report.paused) {
            let cursor = report
                .playback_start_frame
                .checked_add(u64::try_from(report.playback_frames)?)
                .ok_or("BGM render cursor overflow")?;
            session.bgm.feed(cursor, 256, |command| {
                session.runtime.enqueue_audio(command)
            })?;
        }
        let received = device.host_now()?;
        if received.domain != config.origin.domain {
            return Err("native host domain changed".into());
        }
        if let Some(last) = last_host {
            chronology(received, last)?;
        }
        last_host = Some(received);
        session.discipline.validate_host(received)?;
        let old_len = pending.len();
        let batch = device.acquire(&mut pending)?;
        if batch.closed {
            return Ok(());
        }
        if pending.len() < old_len || pending.len() > MAX_PENDING_INPUT_EVENTS {
            return Err("native adapter violated pending input bounds".into());
        }
        let received = device.host_now()?;
        chronology(received, last_host.unwrap())?;
        last_host = Some(received);
        session.discipline.validate_host(received)?;
        // Validate each newly acquired timestamp once, even while ACKs are pending.
        let mut index = old_len;
        while index < pending.len() {
            let host = point(&pending[index]);
            if host.domain != config.origin.domain || host.timestamp > received.timestamp {
                return Err("native input domain differs or timestamp is ahead of receipt".into());
            }
            if host.timestamp < config.origin.timestamp {
                *session.pre_origin_inputs = session
                    .pre_origin_inputs
                    .checked_add(1)
                    .ok_or("pre-origin input counter overflow")?;
                pending.remove(index);
                continue;
            }
            chronology(host, last_acquired)?;
            last_acquired = host;
            session.discipline.validate_host(host)?;
            session.delivery.observe(host, received)?;
            index += 1;
        }
        if (config.logical_schedule || config.pause_supported)
            && (session.pause.last_render_report().is_none()
                || matches!(
                    session.pause.phase(),
                    PausePhase::Pausing | PausePhase::Resuming
                ))
        {
            std::thread::sleep(WallDuration::from_millis(1));
            continue;
        }
        if let Some(at) = resume_boundary {
            while pending
                .front()
                .is_some_and(|event| point(event).timestamp < at.timestamp)
            {
                keyboard.observe_paused(pending.pop_front().unwrap())?;
            }
            if batch.backlog || received.timestamp < at.timestamp {
                std::thread::sleep(WallDuration::from_millis(1));
                continue;
            }
            // Reconciliation precedes every post-resume input, including equal time.
            chronology(at, last_operation)?;
            for event in keyboard.resume(at)? {
                last_song = process(device, &mut session, config, event)?;
            }
            last_operation = at;
            resume_boundary = None;
            player::publish_pause(PauseState::Running);
        }
        while let Some(event) = pending.pop_front() {
            let host = point(&event);
            if paused_boundary.is_some_and(|at| host.timestamp >= at.timestamp) {
                keyboard.observe_paused(event)?;
                continue;
            }
            if end_boundary.is_some_and(|end| host.timestamp >= end.host.timestamp) {
                continue;
            }
            chronology(host, last_operation)?;
            if config.pause_supported && !keyboard.accept(&event)? {
                continue;
            }
            last_song = process(device, &mut session, config, event)?;
            last_operation = host;
        }
        if !batch.backlog {
            if let Some(at) = paused_boundary.filter(|_| !pause_committed) {
                if received.timestamp >= at.timestamp {
                    chronology(at, last_operation)?;
                    let audio_at = schedule(device, &session, config)?;
                    let report = session.runtime.advance_to(at, &ExplicitDomains, audio_at)?;
                    last_song = report.song_time;
                    publish(&mut session, report)?;
                    last_operation = at;
                    pause_committed = true;
                    player::publish_pause(PauseState::Paused);
                }
            }
        }
        if (session.pause.phase() == PausePhase::Paused && !end_rendered)
            || resume_boundary.is_some()
        {
            std::thread::sleep(WallDuration::from_millis(1));
            continue;
        }
        let now = device.host_now()?;
        chronology(now, last_host.unwrap())?;
        last_host = Some(now);
        session.discipline.validate_host(now)?;
        if now.timestamp < config.origin.timestamp {
            std::thread::sleep(WallDuration::from_millis(1));
            continue;
        }
        if let DisciplineUpdate::Applied {
            base_rate_ppm,
            correction_ppm,
            applied_rate_ppm,
            phase_error_ns,
            limited,
        } = session
            .discipline
            .update(now, session.runtime.transport_mut())?
        {
            println!(
                "discipline measured={base_rate_ppm:+}ppm correction={correction_ppm:+}ppm applied={applied_rate_ppm:+}ppm phase={phase_error_ns}ns limited={limited} quality={:?}",
                session.discipline.quality()
            );
        }
        if let Some(at) = watermark(
            config.origin,
            last_operation,
            now,
            config.advance_lag,
            batch.backlog,
        )? {
            let audio_at = schedule(device, &session, config)?;
            let report = session.runtime.advance_to(at, &ExplicitDomains, audio_at)?;
            last_song = report.song_time;
            last_operation = at;
            let second = last_song.as_nanos().div_euclid(1_000_000_000);
            if last_progress != Some(second) {
                println!("logical song={}ns", last_song.as_nanos());
                last_progress = Some(second);
            }
            publish(&mut session, report)?;
        }
        if finite_done(
            config,
            end_boundary,
            last_operation,
            last_song,
            batch.backlog,
            resume_boundary.is_some(),
        ) {
            player::publish_section_end(config.end_song.expect("finite endpoint admitted"));
            return Ok(());
        }
        if !batch.backlog
            && pending.is_empty()
            && resume_boundary.is_none()
            && session.pause.phase() == PausePhase::Running
        {
            if let Some(completion) = session.completion.as_mut() {
                if completion.observe(
                    session.runtime.judge(),
                    last_song,
                    session.bgm.report(),
                    device.render_report()?,
                    session.discipline.latest_pair().map(|pair| pair.source),
                )? {
                    return Ok(());
                }
            }
        }
        std::thread::sleep(WallDuration::from_millis(1));
    }
    Ok(())
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::{
        audio::*,
        input::{
            Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
            EventMeta, GameControlId, PhysicalControlId,
        },
        judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
        replay::{ReplayOperation, codec::ReplayCodecLimits},
        runtime::SoundBinding,
        transport::Rate,
        transport::Transport,
    };
    fn point(domain: u32, ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(domain),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn input(ns: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
        PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(1), point(1, ns), sequence),
            control: PhysicalControlId::keyboard(4),
            state,
        })
    }
    struct Device {
        mixer: Mixer,
        report: Option<RenderReport>,
        step: u64,
        pcm: Vec<f32>,
        backlog: bool,
        pause_scenario: bool,
        viewer: Option<player::PlayerViewer>,
        seeded: usize,
        fallback: usize,
        invalid: bool,
    }
    impl NativeGameplayDevice for Device {
        fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
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
        fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
            Ok(self.report)
        }
        fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
            Ok(point(1, self.step as i64 * 10_000_000))
        }
        fn acquire(
            &mut self,
            events: &mut VecDeque<PhysicalInputEvent>,
        ) -> NativeGameplayResult<InputBatch> {
            if self.pause_scenario {
                let viewer = self.viewer.as_ref().unwrap();
                match self.step {
                    1 => {
                        retain_input(events, input(10_000_000, 1, ButtonState::Down))?;
                        viewer.request_pause(true);
                    }
                    2 => retain_input(events, input(20_000_000, 2, ButtonState::Up))?,
                    3 => viewer.request_pause(false),
                    5 => retain_input(events, input(40_000_000, 3, ButtonState::Down))?,
                    6 => retain_input(events, input(45_000_000, 4, ButtonState::Up))?,
                    _ => {}
                }
                Ok(InputBatch {
                    backlog: self.step == 5,
                    closed: self.step >= 7,
                })
            } else {
                if self.step == 2 {
                    retain_input(
                        events,
                        input(
                            if self.invalid { 30_000_000 } else { 20_000_000 },
                            1,
                            ButtonState::Down,
                        ),
                    )?;
                }
                Ok(InputBatch {
                    backlog: self.backlog && self.step == 2,
                    closed: self.step >= 4,
                })
            }
        }
        fn observe_end(
            &mut self,
            end: &mut NativeEnd,
            discipline: &PresentationDiscipline,
            report: Option<RenderReport>,
        ) -> NativeGameplayResult<Option<EndBoundary>> {
            Ok(end.observe(report, discipline.latest_pair().unwrap())?)
        }
        fn seed_resume(
            &mut self,
            discipline: &mut PresentationDiscipline,
            reference: ClockPair,
        ) -> NativeGameplayResult<()> {
            self.seeded += 1;
            discipline.observe_clock_pair(reference)?;
            Ok(())
        }
        fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
            self.fallback += 1;
            Ok(point(2, self.step as i64 * 10_000_000))
        }
    }
    struct Fixture {
        source: beatkernel_bms::BmsChart,
        device: Device,
        runtime: SoloRuntime,
        bgm: BgmFeeder,
        discipline: PresentationDiscipline,
        pause: NativePause,
        end: Option<NativeEnd>,
        completion: Option<SongCompletion>,
        capture: Option<LiveReplayCapture>,
        competition: Option<LiveCompetition>,
        delivery: InputDeliveryTelemetry,
        pre: u64,
    }
    fn limits() -> ReplayCodecLimits {
        ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 4096).unwrap()).unwrap()
    }
    impl Fixture {
        fn new(finite: bool, pause_window: bool) -> Self {
            let source = beatkernel_bms::parse(
                "#BPM 3000\n#WAV01 original.wav\n#00011:00010000\n",
                Default::default(),
            )
            .unwrap();
            let compiled = source.compile().unwrap();
            let window = if pause_window {
                Duration::from_nanos(10_000_000)
            } else {
                Duration::ZERO
            };
            let profile = JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(1),
                    early: window,
                    late: window,
                }],
                Duration::ZERO,
            )
            .unwrap();
            let judge = JudgeEngine::new(compiled.chart.clone(), source.rules(), profile).unwrap();
            let capture = Some(LiveReplayCapture::new(&judge, ClockDomainId(1), limits()).unwrap());
            let bindings = BindingMap::from_bindings([Binding {
                device: DeviceSelector::Any,
                physical: PhysicalControlId::keyboard(4),
                game_control: GameControlId(0x11),
            }])
            .unwrap();
            let format = AudioFormat::new(1000, 1).unwrap();
            let pcm_limits = PcmLimits::new(64, 256, 1).unwrap();
            let mut bank = SampleBank::new(format, pcm_limits).unwrap();
            bank.insert(
                SampleId(1),
                PcmSample::new(format, vec![0.25, 0.5], pcm_limits).unwrap(),
            )
            .unwrap();
            let (producer, consumer) = command_queue(8).unwrap();
            let config = MixerConfig::new(
                format,
                ClockDomainId(2),
                Timestamp::ZERO,
                AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
            );
            let mixer = Mixer::new(
                if finite {
                    config.with_playback_end_frame(10)
                } else {
                    config
                },
                bank,
                consumer,
            )
            .unwrap();
            let mut runtime = SoloRuntime::new(
                ClockDomainId(1),
                ClockDomainId(2),
                Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
                bindings,
                judge,
                producer,
                vec![SoundBinding {
                    object: compiled.chart.objects()[0].id,
                    stage: beatkernel::judge::JudgeStage::Instant,
                    sample: SampleId(1),
                    voice: VoiceId(1),
                    gain: 1.0,
                }],
                8,
            )
            .unwrap();
            if finite {
                runtime
                    .set_song_end(Timestamp::from_nanos(10_000_000))
                    .unwrap();
            }
            let mut discipline = PresentationDiscipline::new(
                DisciplineConfig::default(),
                point(2, 0),
                ClockDomainId(1),
                Timestamp::ZERO,
            )
            .unwrap();
            let initial = ClockPair {
                source: point(2, 0),
                target: point(1, 0),
            };
            discipline.observe_clock_pair(initial).unwrap();
            let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 1000).unwrap();
            let end = if finite {
                pause = pause.with_playback_end_frame(10).unwrap();
                let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 1000, 10).unwrap();
                end.observe(None, initial).unwrap();
                Some(end)
            } else {
                None
            };
            let bgm = BgmFeeder::new(
                Vec::new(),
                crate::bgm::BgmConfig {
                    output_origin: point(2, 0),
                    sample_rate: 1000,
                    preroll: Duration::ZERO,
                    lookahead: Duration::from_nanos(1_000_000_000),
                    max_pending: 4,
                },
            )
            .unwrap();
            Self {
                source,
                device: Device {
                    mixer,
                    report: None,
                    step: 0,
                    pcm: Vec::new(),
                    backlog: true,
                    pause_scenario: false,
                    viewer: None,
                    seeded: 0,
                    fallback: 0,
                    invalid: false,
                },
                runtime,
                bgm,
                discipline,
                pause,
                end,
                completion: None,
                capture,
                competition: None,
                delivery: InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap(),
                pre: 0,
            }
        }
        fn run(&mut self, finite: bool, logical: bool) -> NativeGameplayResult<()> {
            let pause_supported = self.device.pause_scenario;
            run_gameplay(
                &mut self.device,
                NativeGameplaySession {
                    runtime: &mut self.runtime,
                    bgm: &mut self.bgm,
                    discipline: &mut self.discipline,
                    pause: &mut self.pause,
                    end: &mut self.end,
                    completion: &mut self.completion,
                    capture: &mut self.capture,
                    competition: &mut self.competition,
                    delivery: &mut self.delivery,
                    pre_origin_inputs: &mut self.pre,
                },
                NativeGameplayConfig {
                    origin: point(1, 0),
                    stream_origin: point(2, 0),
                    playback_origin: point(2, 0),
                    song_origin: Timestamp::ZERO,
                    sample_rate: 1000,
                    end_song: finite.then_some(Timestamp::from_nanos(10_000_000)),
                    advance_lag: Duration::from_nanos(10_000_000),
                    seconds: None,
                    pause_supported,
                    logical_schedule: logical,
                },
            )
        }
    }
    #[test]
    fn one_pump_preserves_real_runtime_pcm_capture_and_backlog_watermark() {
        for logical in [true, false] {
            let mut fixture = Fixture::new(false, false);
            fixture.run(false, logical).unwrap();
            assert_eq!(&fixture.device.pcm[20..23], &[0.25, 0.5, 0.0]);
            let capture = fixture.capture.as_ref().unwrap();
            assert_eq!(
                capture
                    .records()
                    .iter()
                    .map(|r| r.song_time.as_nanos())
                    .collect::<Vec<_>>(),
                vec![0, 20_000_000, 20_000_000]
            );
            assert_eq!(fixture.delivery.observed_events(), 1);
            assert_eq!(fixture.device.fallback > 0, !logical);
            assert!(
                capture
                    .records()
                    .iter()
                    .any(|record| matches!(record.operation, ReplayOperation::Input(_)))
            );
            let file = beatkernel::replay::codec::ReplayFile::new(
                capture.header().clone(),
                capture.records().to_vec(),
            );
            crate::replay_playback::reconstruct(&fixture.source, file, limits()).unwrap();
        }
    }
    #[test]
    fn finite_prefix_requires_drained_actual_frontier_and_keeps_later_note_pending() {
        let mut fixture = Fixture::new(true, false);
        fixture.run(true, true).unwrap();
        assert_eq!(fixture.device.step, 3);
        assert_eq!(fixture.delivery.observed_events(), 1);
        assert_eq!(fixture.runtime.telemetry().counters().inputs, 0);
        assert_eq!(
            fixture
                .capture
                .as_ref()
                .unwrap()
                .records()
                .last()
                .unwrap()
                .song_time,
            Timestamp::from_nanos(10_000_000)
        );
        assert_eq!(
            fixture
                .runtime
                .judge()
                .state(beatkernel::chart::ObjectId(1)),
            Some(beatkernel::interaction::InteractionState::Pending)
        );
    }
    #[test]
    fn resume_reseeds_adapter_then_reconciles_before_backlogged_post_resume_input() {
        let (publisher, viewer) = player::channel();
        let mut fixture = Fixture::new(false, true);
        fixture.device.pause_scenario = true;
        fixture.device.viewer = Some(viewer);
        player::with_publisher(publisher, || {
            fixture.run(false, true).map_err(|error| error.to_string())
        })
        .unwrap();
        assert_eq!(fixture.device.seeded, 1);
        let records = fixture.capture.as_ref().unwrap().records();
        let releases = records
            .iter()
            .filter_map(|record| {
                if let ReplayOperation::Input(input) = &record.operation {
                    if let PhysicalInputEvent::Button(event) = &input.physical {
                        if event.state == ButtonState::Up {
                            return Some((
                                event.meta.timestamp.as_nanos(),
                                event.meta.original_clock_point,
                            ));
                        }
                    }
                }
                None
            })
            .collect::<Vec<_>>();
        assert_eq!(releases[0], (40_000_000, Some(point(1, 20_000_000))));
        assert_eq!(releases[1].0, 45_000_000);
        assert_eq!(fixture.delivery.observed_events(), 4);
        assert_eq!(fixture.pause.phase(), PausePhase::Running);
    }
    #[test]
    fn future_input_fails_before_runtime_and_preserves_captured_prefix() {
        let mut fixture = Fixture::new(false, false);
        fixture.device.invalid = true;
        assert!(fixture.run(false, true).is_err());
        assert_eq!(fixture.capture.as_ref().unwrap().records().len(), 1);
        assert_eq!(fixture.delivery.observed_events(), 0);
    }
    #[test]
    fn watermark_uses_wide_bounds_lag_and_backlog() {
        assert_eq!(
            watermark(
                point(1, 0),
                point(1, 10),
                point(1, 20),
                Duration::from_nanos(15),
                false
            )
            .unwrap(),
            Some(point(1, 10))
        );
        assert_eq!(
            watermark(
                point(1, i64::MIN),
                point(1, i64::MIN),
                point(1, i64::MAX),
                Duration::from_nanos(1_000_000_000),
                false
            )
            .unwrap(),
            Some(point(1, i64::MAX - 1_000_000_000))
        );
        assert!(
            watermark(point(1, 0), point(1, 0), point(1, 20), Duration::ZERO, true)
                .unwrap()
                .is_none()
        );
        assert!(
            watermark(
                point(1, 30),
                point(1, 30),
                point(1, 20),
                Duration::ZERO,
                false
            )
            .unwrap()
            .is_none()
        );
        assert!(watermark(point(1, 0), point(1, 0), point(2, 0), Duration::ZERO, false).is_err());
    }
    #[test]
    fn bounded_retention_keeps_order_and_error_preserves_admitted_prefix() {
        let mut pending = VecDeque::new();
        for sequence in 0..MAX_PENDING_INPUT_EVENTS {
            retain_input(
                &mut pending,
                input(sequence as i64, sequence as u64, ButtonState::Down),
            )
            .unwrap();
        }
        assert!(retain_input(&mut pending, input(0, u64::MAX, ButtonState::Up)).is_err());
        assert_eq!(pending.len(), MAX_PENDING_INPUT_EVENTS);
        assert_eq!(pending.front().unwrap().meta().sequence, 0);
        assert_eq!(
            pending.back().unwrap().meta().sequence,
            MAX_PENDING_INPUT_EVENTS as u64 - 1
        );
    }
}
