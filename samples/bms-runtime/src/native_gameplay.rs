//! One solo gameplay owner; native adapters acquire evidence and own cleanup.
use crate::{
    bgm::{BgmFeedReport, BgmFeeder},
    completion::SongCompletion,
    gameplay_competition::SoloCompetitionPort,
    gameplay_presentation::GameplayPresentationPort,
    gauge::{BmsGauge, GaugeError, GaugeProfile},
    live_pause::{
        LivePauseBoundary, prepare_live_transport, update_live_pause, validate_pre_pause_input,
    },
    local_runtime::SoloRuntime,
    native_end::{EndBoundary, NativeEnd},
    native_audio::{NativeStopBarrier, finite_terminal_output_ready, validate_stop_evidence},
    native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost, PauseState},
    native_pump_control::{NativePumpControl, NativePumpDeadline},
    offline::OwnedStopEvidence,
    playback_pause::{NativePause, PauseKeyboard, PausePhase},
    play_result::{CompletedPlayResult, CompletedSoloPublicationError},
    replay_capture::{CaptureError, LiveReplayCapture},
};
use beatkernel::{
    audio::RenderReport,
    input::PhysicalInputEvent,
    runtime::RuntimeReport,
    telemetry::InputDeliveryTelemetry,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use beatkernel::time::presentation::DisciplineUpdate;
#[cfg(test)]
use beatkernel::time::presentation::DisciplineConfig;
#[cfg(test)]
use beatkernel::time::ClockPair;
#[cfg(test)]
use beatkernel_platform::audio::presentation::discipline::PresentationDiscipline;
use std::{collections::VecDeque, error::Error, fmt, time::Duration as WallDuration};

pub use crate::native_gameplay_bridge::{
    NativeGameplayDevice, NativeGameplaySession, run_gameplay, run_gameplay_with_control,
    run_gameplay_with_result, run_gameplay_with_result_and_score,
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
/// Borrowed gameplay state with an explicitly selected competition observer.
pub struct GameplaySession<'a, S, P> {
    pub runtime: &'a mut SoloRuntime,
    pub gauge: &'a mut BmsGauge,
    pub bgm: &'a mut BgmFeeder,
    pub discipline: &'a mut P,
    pub pause: &'a mut NativePause,
    pub end: &'a mut Option<NativeEnd>,
    pub completion: &'a mut Option<SongCompletion>,
    pub capture: &'a mut Option<LiveReplayCapture>,
    pub competition: &'a mut Option<S>,
    pub delivery: &'a mut InputDeliveryTelemetry,
    pub pre_origin_inputs: &'a mut u64,
}

/// Independent observation failures retain the complete committed operation.
#[derive(Debug)]
pub struct NativeReportObservationError {
    pub report: RuntimeReport,
    pub gauge_error: Option<GaugeError>,
    pub capture_error: Option<CaptureError>,
    pub competition_error: Option<Box<dyn Error>>,
    pub presentation_error: Option<Box<dyn Error>>,
    pub stop_evidence_error: Option<&'static str>,
}
impl fmt::Display for NativeReportObservationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "native report observation: judge {:?}, {} audio failures, gauge {:?}, capture {:?}, competition {:?}, presentation {:?}, Stop evidence {:?}",
            self.report.judge_error,
            self.report.audio_failures.len(),
            self.gauge_error,
            self.capture_error,
            self.competition_error,
            self.presentation_error,
            self.stop_evidence_error,
        )
    }
}
impl Error for NativeReportObservationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        if let Some(error) = &self.gauge_error {
            return Some(error);
        }
        if let Some(error) = &self.capture_error {
            return Some(error);
        }
        if let Some(error) = self.competition_error.as_deref() {
            return Some(error);
        }
        if let Some(error) = self.presentation_error.as_deref() {
            return Some(error);
        }
        self.report
            .judge_error
            .as_ref()
            .map(|error| error as &dyn Error)
    }
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
fn schedule<D: crate::gameplay_presentation::GameplayDevice, S>(
    device: &mut D,
    session: &GameplaySession<'_, S, D::Presentation>,
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
#[cfg(test)]
fn publish(
    session: &mut NativeGameplaySession<'_>,
    report: RuntimeReport,
) -> NativeGameplayResult<()> {
    publish_with_stops(session, report, &mut OwnedStopEvidence::default())
}
#[cfg(test)]
fn publish_with_stops(
    session: &mut NativeGameplaySession<'_>,
    report: RuntimeReport,
    evidence: &mut OwnedStopEvidence,
) -> NativeGameplayResult<()> {
    publish_with_host(
        session,
        report,
        evidence,
        &mut crate::native_gameplay_bridge::PlayerGameplayHost,
    )
}
fn publish_with_host<S: SoloCompetitionPort, P, H: NativeGameplayHost>(
    session: &mut GameplaySession<'_, S, P>,
    mut report: RuntimeReport,
    evidence: &mut OwnedStopEvidence,
    host_port: &mut H,
) -> NativeGameplayResult<()> {
    let was_fenced = session.runtime.gameplay_fence().is_some();
    let gauge_error = session
        .gauge
        .observe(&report.judge_events, &report.hazard_events)
        .err();
    let capture_error = if was_fenced {
        None
    } else {
        session
            .capture
            .as_mut()
            .and_then(|capture| capture.record_report(&report).err())
    };
    let competition_error = session
        .competition
        .as_mut()
        .and_then(|competition| competition.observe(&report).err());
    let presentation_error = host_port.publish_report(&report).err();
    let mut stop_evidence_error = None;
    if session.gauge.snapshot().failure.is_some() {
        session.runtime.fence_gameplay();
        if let Some(stops) = session
            .runtime
            .fence_gameplay_sounds(report.audio_at.timestamp)
        {
            stop_evidence_error = evidence.record_admitted(&stops.commands).err();
            if !stops.commands.is_empty() {
                if let Some(completion) = session.completion.as_mut() {
                    completion.reset_drain();
                }
            }
            report.audio_commands.extend(stops.commands);
            report.audio_failures.extend(stops.failures);
        }
    }
    host_port.diagnostic(NativeGameplayDiagnostic::SoloReport(&report));
    if report.judge_error.is_some()
        || !report.audio_failures.is_empty()
        || gauge_error.is_some()
        || capture_error.is_some()
        || competition_error.is_some()
        || presentation_error.is_some()
        || stop_evidence_error.is_some()
    {
        return Err(Box::new(NativeReportObservationError {
            report,
            gauge_error,
            capture_error,
            competition_error,
            presentation_error,
            stop_evidence_error,
        }));
    }
    Ok(())
}
fn process<
    D: crate::gameplay_presentation::GameplayDevice,
    S: SoloCompetitionPort,
    H: NativeGameplayHost,
>(
    device: &mut D,
    session: &mut GameplaySession<'_, S, D::Presentation>,
    config: NativeGameplayConfig,
    event: PhysicalInputEvent,
    evidence: &mut OwnedStopEvidence,
    host_port: &mut H,
) -> NativeGameplayResult<Timestamp> {
    let at = schedule(device, session, config)?;
    let report = session.runtime.process_input(event, &ExplicitDomains, at)?;
    let song = report.song_time;
    publish_with_host(session, report, evidence, host_port)?;
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
    finite_frontier_ready(
        config.end_song.is_some_and(|end| song >= end),
        boundary,
        last,
        backlog,
        resuming,
    )
}
fn finite_frontier_ready(
    gameplay_ready: bool,
    boundary: Option<EndBoundary>,
    last: ClockPoint,
    backlog: bool,
    resuming: bool,
) -> bool {
    gameplay_ready
        && boundary.is_some_and(|boundary| {
            boundary.host.domain == last.domain && last.timestamp >= boundary.host.timestamp
        })
        && !backlog
        && !resuming
}
fn finite_done_with_terminal(
    config: NativeGameplayConfig,
    boundary: Option<EndBoundary>,
    last: ClockPoint,
    song: Timestamp,
    backlog: bool,
    resuming: bool,
    numeric_terminal: bool,
    bgm: BgmFeedReport,
    rendered: Option<RenderReport>,
    admitted_commands: u64,
) -> bool {
    if !numeric_terminal {
        return finite_done(config, boundary, last, song, backlog, resuming);
    }
    finite_frontier_ready(config.end_song.is_some(), boundary, last, backlog, resuming)
        && finite_terminal_output_ready(bgm, rendered, admitted_commands)
}

pub fn run_gameplay_with_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
>(
    device: &mut D,
    session: GameplaySession<'_, S, D::Presentation>,
    config: NativeGameplayConfig,
    control: &mut C,
    host_port: &mut H,
) -> NativeGameplayResult<()> {
    run_gameplay_with_result_and_ports(device, session, config, control, host_port).map(|_| ())
}

fn complete_gameplay<S: SoloCompetitionPort, P, H: NativeGameplayHost>(
    session: &mut GameplaySession<'_, S, P>,
    config: NativeGameplayConfig,
    host: &mut H,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    if host.cancelled() {
        return Ok(None);
    }
    let result =
        CompletedPlayResult::from_completed(config.song_origin, config.end_song, session.gauge);
    if let Some(competition) = session.competition.as_mut() {
        competition.mark_native_completed();
    }
    if let Some(end) = config.end_song {
        host.publish_section_end(end);
    }
    if let Err(cause) = host.publish_completed_solo(result) {
        return Err(Box::new(CompletedSoloPublicationError { result, cause }));
    }
    Ok(Some(result))
}

/// Runs the actual pump with explicit device, clock/wait and host effects.
/// Neither the control deadline nor cancellation is successful song completion.
pub fn run_gameplay_with_result_and_score_and_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
>(
    device: &mut D,
    session: GameplaySession<'_, S, D::Presentation>,
    config: NativeGameplayConfig,
    control: &mut C,
    host_port: &mut H,
    score: &mut crate::competition::ScoreSummary,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    if score != &crate::competition::ScoreSummary::default() {
        return Err("native scored gameplay requires a default initial score".into());
    }
    let mut observer = crate::native_gameplay_host::NativeScoreHost::new(host_port, score);
    run_gameplay_with_result_and_ports(device, session, config, control, &mut observer)
}

pub fn run_gameplay_with_result_and_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
>(
    device: &mut D,
    mut session: GameplaySession<'_, S, D::Presentation>,
    mut config: NativeGameplayConfig,
    control: &mut C,
    host_port: &mut H,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    if session.gauge.profile() != &GaugeProfile::default() {
        return Err("native gameplay requires the default recorded gauge policy".into());
    }
    if config.origin.domain == config.stream_origin.domain
        || config.stream_origin.domain != config.playback_origin.domain
        || config.playback_origin.timestamp < config.stream_origin.timestamp
        || config.sample_rate == 0
        || config.sample_rate > 1_000_000_000
        || !(0..=1_000_000_000).contains(&config.advance_lag.as_nanos())
    {
        return Err("invalid native gameplay clocks/rate/lag".into());
    }
    let mut deadline =
        NativePumpDeadline::new(control, config.seconds, "native gameplay deadline overflow")?;
    let mut pending = VecDeque::new();
    pending.try_reserve_exact(4096)?;
    let mut last_acquired = config.origin;
    let mut last_operation = config.origin;
    let mut last_song = config.song_origin;
    let mut last_progress = None;
    let mut last_host = None;
    let mut keyboard = PauseKeyboard::new();
    let mut paused_boundary: Option<LivePauseBoundary> = None;
    let mut resume_boundary: Option<ClockPoint> = None;
    let mut pause_committed = false;
    let mut end_boundary = None;
    let mut end_rendered = false;
    let mut pause_announced = false;
    let mut stop_evidence = OwnedStopEvidence::default();
    let mut stop_barrier = NativeStopBarrier::default();
    while !host_port.cancelled() && deadline.active(control)? {
        host_port.retry_pause_publication();
        device.observe(session.discipline)?;
        let reference = session
            .discipline
            .latest_pair()
            .ok_or("gameplay requires native clock relation")?;
        let rendered = device.render_report()?;
        validate_stop_evidence(rendered, &stop_evidence)?;
        if let Some(end) = session.end.as_mut() {
            end_rendered |=
                rendered.is_some_and(|report| report.playback_end_physical_frame.is_some());
            if let Some(boundary) = device.observe_end(end, session.discipline, rendered)? {
                end_boundary = Some(boundary);
            }
        }
        if config.logical_schedule || config.pause_supported {
            let desired = (config.pause_supported
                && !end_rendered
                && (session.pause.phase() == PausePhase::Running || pause_committed)
                && resume_boundary.is_none())
            .then(|| host_port.pause_requested())
            .filter(|desired| *desired || !device.output_replacement_pending());
            let update = update_live_pause(
                session.pause,
                device.pause_observation(reference)?,
                rendered,
                desired,
                config.song_origin,
                config.sample_rate,
            )?;
            if update.observed && config.pause_supported && !pause_announced {
                pause_announced = true;
                host_port.publish_pause(PauseState::Running);
            }
            if let Some(desired) = update.requested {
                session.runtime.request_audio_pause(desired);
                host_port.publish_pause(if desired {
                    PauseState::Pausing
                } else {
                    PauseState::Resuming
                });
            }
            if let Some(boundary) = update.boundary {
                host_port.diagnostic(NativeGameplayDiagnostic::Pause {
                    local: false,
                    boundary,
                });
                if boundary.paused {
                    if !end_rendered {
                        let transport = prepare_live_transport(
                            session.runtime.transport_mut(),
                            boundary,
                            last_song,
                        )?;
                        *session.runtime.transport_mut() = transport;
                        paused_boundary = Some(boundary);
                        pause_committed = false;
                    }
                } else {
                    let transport = prepare_live_transport(
                        session.runtime.transport_mut(),
                        boundary,
                        last_song,
                    )?;
                    let mut discipline = session.discipline.restart_for_resume(
                        config.stream_origin,
                        config.playback_origin,
                        config.origin.domain,
                        session.pause.song_origin_for_presentation(
                            config.song_origin,
                            config.playback_origin,
                        )?,
                    )?;
                    device.seed_resume(&mut discipline, reference)?;
                    if discipline.latest_pair().is_none() {
                        return Err("resume presentation seed has no accepted observation".into());
                    }
                    *session.runtime.transport_mut() = transport;
                    *session.discipline = discipline;
                    resume_boundary = Some(boundary.at);
                    paused_boundary = None;
                    pause_committed = false;
                }
            }
        }
        crate::native_audio::feed_rendered(session.bgm, device.render_report()?, |command| {
            session.runtime.enqueue_audio(command)
        })?;
        let received = device.host_now()?;
        if received.domain != config.origin.domain {
            return Err("native host domain changed".into());
        }
        if let Some(last) = last_host {
            chronology(received, last)?;
        }
        last_host = Some(received);
        let frozen_pause = paused_boundary
            .filter(|_| {
                pause_committed && session.pause.phase() == PausePhase::Paused && !end_rendered
            })
            .map(|boundary| boundary.at);
        crate::gameplay_presentation::validate_gameplay_host(
            device,
            session.discipline,
            received,
            frozen_pause,
        )?;
        let old_len = pending.len();
        let batch = device.acquire(&mut pending)?;
        if batch.closed {
            return Ok(None);
        }
        if pending.len() < old_len || pending.len() > MAX_PENDING_INPUT_EVENTS {
            return Err("native adapter violated pending input bounds".into());
        }
        let received = device.host_now()?;
        chronology(received, last_host.unwrap())?;
        last_host = Some(received);
        crate::gameplay_presentation::validate_gameplay_host(
            device,
            session.discipline,
            received,
            frozen_pause,
        )?;
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
            crate::gameplay_presentation::validate_gameplay_host(
                device,
                session.discipline,
                host,
                frozen_pause,
            )?;
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
            control.wait(WallDuration::from_millis(1))?;
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
                control.wait(WallDuration::from_millis(1))?;
                continue;
            }
            // Reconciliation precedes every post-resume input, including equal time.
            chronology(at, last_operation)?;
            for event in keyboard.resume(at)? {
                last_song = process(
                    device,
                    &mut session,
                    config,
                    event,
                    &mut stop_evidence,
                    host_port,
                )?;
            }
            last_operation = at;
            resume_boundary = None;
            host_port.publish_pause(PauseState::Running);
        }
        while let Some(event) = pending.pop_front() {
            let host = point(&event);
            if paused_boundary.is_some_and(|boundary| host.timestamp >= boundary.at.timestamp) {
                keyboard.observe_paused(event)?;
                continue;
            }
            if end_boundary.is_some_and(|end| host.timestamp >= end.host.timestamp) {
                continue;
            }
            chronology(host, last_operation)?;
            if let Some(boundary) = paused_boundary {
                validate_pre_pause_input(session.runtime.transport_mut(), boundary, host)?;
            }
            if config.pause_supported && !keyboard.accept(&event)? {
                continue;
            }
            last_song = process(
                device,
                &mut session,
                config,
                event,
                &mut stop_evidence,
                host_port,
            )?;
            last_operation = host;
        }
        if !batch.backlog {
            if let Some(boundary) = paused_boundary.filter(|_| !pause_committed) {
                let at = boundary.at;
                if received.timestamp >= at.timestamp {
                    chronology(at, last_operation)?;
                    let audio_at = schedule(device, &session, config)?;
                    let report = session.runtime.advance_to(at, &ExplicitDomains, audio_at)?;
                    last_song = report.song_time;
                    publish_with_host(&mut session, report, &mut stop_evidence, host_port)?;
                    last_operation = at;
                    pause_committed = true;
                    host_port.publish_pause(PauseState::Paused);
                }
            }
        }
        if pause_committed
            && session.pause.phase() == PausePhase::Paused
            && !end_rendered
            && end_boundary.is_none()
            && resume_boundary.is_none()
            && !host_port.cancelled()
        {
            device.publish_paused_output(crate::gameplay_presentation::GameplayOutputContext {
                control: crate::gameplay_presentation::GameplayPauseControl::solo(session.runtime),
                presentation: session.discipline,
                pause: session.pause,
                config: &mut config,
                end: session.end,
            })?;
        }
        if (session.pause.phase() == PausePhase::Paused && !end_rendered)
            || resume_boundary.is_some()
        {
            control.wait(WallDuration::from_millis(1))?;
            continue;
        }
        let now = device.host_now()?;
        chronology(now, last_host.unwrap())?;
        last_host = Some(now);
        session.discipline.validate_host(now)?;
        if now.timestamp < config.origin.timestamp {
            control.wait(WallDuration::from_millis(1))?;
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
            host_port.diagnostic(NativeGameplayDiagnostic::Discipline {
                base_rate_ppm,
                correction_ppm,
                applied_rate_ppm,
                phase_error_ns,
                limited,
                quality: session.discipline.quality(),
            });
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
                host_port.diagnostic(NativeGameplayDiagnostic::SongProgress(last_song));
                last_progress = Some(second);
            }
            publish_with_host(&mut session, report, &mut stop_evidence, host_port)?;
        }
        // Read after this iteration's admissions. A pre-admission idle block
        // cannot establish that newly queued Stops reached the mixer.
        let rendered = device.render_report()?;
        validate_stop_evidence(rendered, &stop_evidence)?;
        let numeric_terminal = session.gauge.snapshot().failure.is_some()
            && session.runtime.gameplay_fence().is_some();
        let stops_rendered = if config.end_song.is_some() {
            stop_barrier.observe(&stop_evidence, rendered)?
        } else {
            true
        };
        if stops_rendered
            && finite_done_with_terminal(
                config,
                end_boundary,
                last_operation,
                last_song,
                batch.backlog,
                resume_boundary.is_some(),
                numeric_terminal,
                session.bgm.report(),
                rendered,
                session.runtime.admitted_audio_commands(),
            )
        {
            return complete_gameplay(&mut session, config, host_port);
        }
        if config.end_song.is_none()
            && !batch.backlog
            && pending.is_empty()
            && resume_boundary.is_none()
            && session.pause.phase() == PausePhase::Running
        {
            if let Some(completion) = session.completion.as_mut() {
                let presented = session.discipline.latest_pair().map(|pair| pair.source);
                let finished = if numeric_terminal {
                    completion.observe_terminal_ready(
                        true,
                        session.bgm.report(),
                        rendered,
                        presented,
                    )?
                } else {
                    completion.observe(
                        session.runtime.judge(),
                        last_song,
                        session.bgm.report(),
                        rendered,
                        presented,
                    )?
                };
                if finished {
                    return complete_gameplay(&mut session, config, host_port);
                }
            }
        }
        control.wait(WallDuration::from_millis(1))?;
    }
    Ok(None)
}

#[cfg(test)]
mod fixtures {
    use crate::{competition_live::LiveCompetition, player};
    mod competition_port {
        include!("native_solo_competition_port_fixtures.rs");
    }
    mod host {
        include!("native_solo_host_fixtures.rs");
    }
    mod control {
        include!("native_solo_control_fixtures.rs");
    }
    mod failed_terminal {
        include!("native_solo_failed_terminal_fixtures.rs");
    }
    mod sound_stop {
        include!("native_solo_sound_stop_fixtures.rs");
    }
    mod gauge_fence {
        include!("native_solo_gauge_fence_fixtures.rs");
    }
    mod room_completion {
        include!("native_room_completion_fixtures.rs");
    }
    include!("native_gameplay_interval_fixtures.rs");
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
        gauge: BmsGauge,
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
                gauge: BmsGauge::default(),
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
                    gauge: &mut self.gauge,
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

#[cfg(test)]
#[path = "native_scored_play_fixtures.rs"]
mod native_scored_play_fixtures;
