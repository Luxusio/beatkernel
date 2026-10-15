//! One solo gameplay owner; native adapters acquire evidence and own cleanup.
use crate::{
    bgm::{BgmFeedReport, BgmFeeder},
    completion::SongCompletion,
    gameplay_competition::SoloCompetitionPort,
    gameplay_presentation::{AudioTiming, LegacyTiming, PumpTiming},
    gauge::{BmsGauge, GaugeError, GaugeProfile},
    live_pause::{
        prepare_live_audio_transport, prepare_live_transport, update_live_audio_pause,
        update_live_pause, validate_pre_pause_input, AudioLivePauseBoundary, LivePauseBoundary,
    },
    live_speed::LiveSpeedSetup,
    local_input::InputMerger,
    local_runtime::SoloRuntime,
    native_audio::{finite_terminal_output_ready, NativeRenderEvidence, NativeStopBarrier},
    native_audio_presentation::NativeAudioPresentation,
    native_end::{EndBoundary, NativeEnd},
    native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost, PauseState},
    native_pump_control::{NativePumpControl, NativePumpDeadline},
    offline::OwnedStopEvidence,
    play_result::{CompletedPlayResult, CompletedSoloPublicationError},
    playback_pause::{NativePause, PauseKeyboard, PausePhase},
    replay_capture::{CaptureError, LiveReplayCapture},
};
#[cfg(test)]
use beatkernel::time::presentation::DisciplineConfig;
use beatkernel::time::presentation::DisciplineUpdate;
#[cfg(test)]
use beatkernel::time::ClockPair;
use beatkernel::{
    audio::RenderReport,
    input::PhysicalInputEvent,
    runtime::RuntimeReport,
    telemetry::InputDeliveryTelemetry,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
#[cfg(test)]
use beatkernel_platform::audio::presentation::discipline::PresentationDiscipline;
use std::{collections::VecDeque, error::Error, fmt, time::Duration as WallDuration};

pub use crate::native_gameplay_bridge::{
    run_gameplay, run_gameplay_audio_coordinate_with_practice_and_result_and_score,
    run_gameplay_audio_with_policy_and_result_and_score,
    run_gameplay_audio_with_policy_and_result_and_score_and_speed,
    run_gameplay_audio_with_practice_and_result_and_score, run_gameplay_audio_with_result,
    run_gameplay_with_control, run_gameplay_with_policy_and_result_and_score,
    run_gameplay_with_result, run_gameplay_with_result_and_score, NativeAudioGameplaySession,
    NativeGameplayDevice, NativeGameplaySession, NativePracticeRecorder,
};

#[cfg(not(target_arch = "wasm32"))]
pub use crate::native_gameplay_bridge::NativeCollectedInput;

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
    /// Original native drain evidence, published after all covered input.
    pub completed_through: Option<ClockPoint>,
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
/// Audio gameplay clocks together with the original requested chart section identity.
#[derive(Clone, Copy, Debug)]
pub struct AudioGameplayConfig {
    pub gameplay: NativeGameplayConfig,
    pub section_start: Timestamp,
}
/// Explicit source-history binding for original-coordinate audio gameplay.
/// Identity binding does not itself qualify a native observation.
#[derive(Clone, Copy, Debug)]
pub struct CoordinateGameplayConfig {
    pub audio: AudioGameplayConfig,
    pub source_history: crate::gameplay::output::ports::SourcePlaybackIdentity,
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

/// Original-audio solo state with its one retained acquisition merger.
pub struct AudioGameplaySession<'a, S> {
    pub session: GameplaySession<'a, S, NativeAudioPresentation>,
    pub merger: &'a mut InputMerger,
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
/// Validate raw acquisition evidence independently of receipt and gameplay origin.
pub(crate) fn completed_cut(
    cut: Option<ClockPoint>,
    receipt: ClockPoint,
    origin: ClockPoint,
    previous: &mut Option<ClockPoint>,
) -> NativeGameplayResult<Option<ClockPoint>> {
    if let Some(cut) = cut {
        if cut.domain != origin.domain
            || cut.domain != receipt.domain
            || cut.timestamp > receipt.timestamp
            || previous.is_some_and(|last| cut.timestamp < last.timestamp)
        {
            return Err("native completed input cut has invalid domain/future/regression".into());
        }
        *previous = Some(cut);
    }
    Ok(previous.filter(|cut| cut.timestamp >= origin.timestamp))
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
fn schedule<D: crate::gameplay_presentation::GameplayDevice, S, P: PumpTiming<D>>(
    device: &mut D,
    session: &mut GameplaySession<'_, S, P>,
    config: NativeGameplayConfig,
    render_evidence: &mut NativeRenderEvidence,
    stops: &OwnedStopEvidence,
) -> NativeGameplayResult<ClockPoint> {
    if config.logical_schedule {
        let rendered = device
            .render_report()?
            .or_else(|| render_evidence.latest())
            .or_else(|| session.pause.last_render_report());
        let report = render_evidence
            .validate(rendered, stops)?
            .ok_or("mixer playback boundary unavailable for scheduling")?;
        let at = session.pause.scheduling_point(report)?;
        if P::AUDIO
            && session.end.is_some()
            && report.frames != 0
            && report.playback_end_physical_frame.is_some()
        {
            // The callback can reach the immutable endpoint between pump
            // observation and scheduling. Use this same validated receipt.
            session.runtime.seal_gameplay_audio();
        }
        Ok(at)
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
    P: PumpTiming<D>,
>(
    device: &mut D,
    session: &mut GameplaySession<'_, S, P>,
    config: NativeGameplayConfig,
    event: PhysicalInputEvent,
    evidence: &mut OwnedStopEvidence,
    host_port: &mut H,
    render_evidence: &mut NativeRenderEvidence,
) -> NativeGameplayResult<Timestamp> {
    let at = schedule(device, session, config, render_evidence, evidence)?;
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

/// Admits the full selected setup before entering the existing native pump.
pub fn run_gameplay_with_policy_and_result_and_score_and_ports<
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
    policy: &crate::play_policy::ResolvedPlayPolicy,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    let policies = [(crate::local_players::PlayerId(1), policy)];
    crate::native_gameplay_host::validate_play_policy_members(&policies)?;
    crate::native_policy_admission::validate_config(&config)?;
    if score != &crate::competition::ScoreSummary::default()
        || session.runtime.poisoned()
        || session.runtime.gameplay_fence().is_some()
    {
        return Err("selected native gameplay requires a pristine runtime and score".into());
    }
    let competition = session
        .competition
        .as_ref()
        .filter(|port| !port.policy_agnostic());
    let header = competition
        .map(|port| {
            port.expected_policy_header()
                .ok_or("selected native competition has no policy identity")
        })
        .transpose()?;
    crate::native_policy_admission::validate_selected(
        session.runtime.judge(),
        session.gauge,
        policy,
        session.capture.as_ref(),
        header,
        &config,
    )?;
    if policy.gauge() == &GaugeProfile::default() {
        host_port.prepare_play_policies(&policies)?;
    }
    let mut host = crate::native_gameplay_bridge::ResolvedGameplayHost {
        host: host_port,
        policies: &policies,
    };
    run_gameplay_with_result_and_score_and_ports(device, session, config, control, &mut host, score)
}

pub fn run_gameplay_with_result_and_ports<
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
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    let mut timing = LegacyTiming(session.discipline);
    let session = GameplaySession {
        runtime: session.runtime,
        gauge: session.gauge,
        bgm: session.bgm,
        pause: session.pause,
        end: session.end,
        completion: session.completion,
        capture: session.capture,
        competition: session.competition,
        delivery: session.delivery,
        pre_origin_inputs: session.pre_origin_inputs,
        discipline: &mut timing,
    };
    run_gameplay_timed(
        device,
        session,
        None,
        config,
        control,
        host_port,
        None::<RetainedSoloPractice<crate::practice_playback::NoopPracticeRecording>>,
    )
}

struct RetainedSoloPractice<'a, R: crate::practice_playback::PracticeRecordingPort> {
    playback: &'a mut crate::practice_playback::PracticePlayback,
    recording: &'a mut R,
    last_playback_frame: u64,
    completion_template: Option<SongCompletion>,
}

enum PracticeService {
    Waiting,
    Committed { fresh: bool, terminal: bool },
}

/// Preserve the actual presentation proof while sharing the legacy and opt-in
/// pumps. A rate event never becomes a synthetic attempt boundary.
#[derive(Clone, Copy)]
pub(crate) enum NativePracticeBoundary {
    Fixed(crate::native_audio_presentation::PreparedPracticeBoundary),
    Runtime(crate::native_audio_presentation::PreparedPracticeEventBoundary),
    RuntimeRate(crate::native_audio_presentation::PreparedPracticeRateCut),
}

impl NativePracticeBoundary {
    pub(crate) fn pending(
        owner: &mut crate::practice_playback::PracticePlayback,
    ) -> NativeGameplayResult<bool> {
        if owner.rate_state().is_some() {
            Ok(owner.peek_event()?.is_some())
        } else {
            Ok(owner.peek_receipt()?.is_some())
        }
    }

    /// After the projected-event poll, an actual unprojected PCM receipt may
    /// restrict progress. This pin grants no presentation or control authority.
    pub(crate) fn unprojected_source_cut(
        owner: &crate::practice_playback::PracticePlayback,
    ) -> Option<u64> {
        if owner.rate_state().is_none() || owner.history_retention_pins().is_some() {
            return None;
        }
        let (source, target) = owner.queued_history_retention_pins();
        source.filter(|_| target.is_none())
    }

    pub(crate) fn qualify<D: crate::gameplay_presentation::GameplayDevice>(
        owner: &mut crate::practice_playback::PracticePlayback,
        device: &mut D,
        presentation: &NativeAudioPresentation,
        identity: Option<crate::gameplay::output::ports::SourcePlaybackIdentity>,
        attempt: Option<beatkernel::transport::OutputPhaseBasis>,
        now: ClockPoint,
    ) -> NativeGameplayResult<Option<Self>> {
        if owner.rate_state().is_some() {
            let Some(boundary) = owner.qualify_event(presentation, now)? else {
                return Ok(None);
            };
            if matches!(
                boundary.projected_event().event,
                beatkernel::audio::PracticeEvent::Rate(_)
            ) {
                let identity = identity.ok_or("runtime rate lacks original source identity")?;
                let attempt = attempt.ok_or("runtime rate lacks its output phase basis")?;
                let query = boundary
                    .coordinate_query()?
                    .ok_or("runtime rate lacks its original projected query")?;
                let Some(mapping) =
                    device.conditional_playback_mapping_at(presentation, &query, identity)?
                else {
                    return Ok(None);
                };
                Ok(presentation
                    .prepare_practice_rate_cut(boundary, mapping, identity, attempt, now)?
                    .map(Self::RuntimeRate))
            } else {
                Ok(Some(Self::Runtime(boundary)))
            }
        } else {
            Ok(owner.qualify(presentation, now)?.map(Self::Fixed))
        }
    }

    pub(crate) fn receipt(self) -> Option<beatkernel::audio::PracticeReceipt> {
        match self {
            Self::Fixed(boundary) => Some(boundary.receipt()),
            Self::Runtime(boundary) => match boundary.projected_event().event {
                beatkernel::audio::PracticeEvent::Boundary(receipt) => Some(receipt),
                beatkernel::audio::PracticeEvent::Restart(event) => Some(event.boundary),
                beatkernel::audio::PracticeEvent::Rate(_) => None,
            },
            Self::RuntimeRate(_) => None,
        }
    }

    pub(crate) fn restarts_original(self) -> bool {
        match self {
            Self::Fixed(_) | Self::RuntimeRate(_) => false,
            Self::Runtime(boundary) => matches!(
                boundary.projected_event().event,
                beatkernel::audio::PracticeEvent::Restart(event)
                    if event.outcome == beatkernel::audio::PracticeRateOutcome::Applied
            ),
        }
    }

    pub(crate) fn output(self) -> ClockPoint {
        match self {
            Self::Fixed(boundary) => boundary.output(),
            Self::Runtime(boundary) => boundary.output(),
            Self::RuntimeRate(cut) => cut.output(),
        }
    }

    pub(crate) fn host(self) -> ClockPoint {
        match self {
            Self::Fixed(boundary) => boundary.host(),
            Self::Runtime(boundary) => boundary.host(),
            Self::RuntimeRate(cut) => cut.host(),
        }
    }

    pub(crate) fn coordinate_query(
        self,
    ) -> NativeGameplayResult<Option<beatkernel::audio::InterpolatedTargetTime>> {
        match self {
            Self::Fixed(boundary) => boundary.coordinate_query(),
            Self::Runtime(boundary) => boundary.coordinate_query(),
            Self::RuntimeRate(cut) => Ok(Some(cut.coordinate_query())),
        }
    }

    pub(crate) fn ready(
        self,
        presentation: &NativeAudioPresentation,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<bool> {
        match self {
            Self::Fixed(boundary) => presentation.practice_boundary_ready(&boundary, now, merger),
            Self::Runtime(boundary) => {
                presentation.practice_event_boundary_ready(&boundary, now, merger)
            }
            Self::RuntimeRate(cut) => presentation.practice_rate_cut_ready(&cut, now, merger),
        }
    }

    pub(crate) fn prepare<H: NativeGameplayHost>(
        self,
        owner: &crate::practice_playback::PracticePlayback,
        host: &mut H,
    ) -> NativeGameplayResult<crate::practice_playback::PreparedPracticeCommit> {
        match self {
            Self::Fixed(boundary) => owner.prepare_boundary(&boundary, host),
            Self::Runtime(boundary) => owner.prepare_event_boundary(&boundary, host),
            Self::RuntimeRate(_) => Err("runtime rate requires same-attempt preparation".into()),
        }
    }

    pub(crate) fn commit(
        self,
        presentation: &mut NativeAudioPresentation,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> NativeGameplayResult<()> {
        match self {
            Self::Fixed(boundary) => presentation.commit_practice_boundary(boundary, now, merger),
            Self::Runtime(boundary) => {
                presentation.commit_practice_event_boundary(boundary, now, merger)
            }
            Self::RuntimeRate(cut) => presentation.commit_practice_rate_cut(cut, now, merger),
        }
    }
}

/// The same audio pump with an installed recurring program and retained output.
/// Archives retired attempts; the session retains its final capture on every
/// outcome for the native finisher's replay, result and score publication.
/// An earlier pump failure remains primary if capability revocation also fails.
pub fn run_gameplay_audio_with_practice_and_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
    R: crate::practice_playback::PracticeRecordingPort,
>(
    device: &mut D,
    audio: AudioGameplaySession<'_, S>,
    config: AudioGameplayConfig,
    control: &mut C,
    host: &mut H,
    practice: &mut crate::practice_playback::PracticePlayback,
    recording: &mut R,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    run_gameplay_audio_with_practice_and_ports_impl(
        device, audio, config, control, host, practice, recording, None,
    )
}

/// Retained practice using its explicit original source history and cold setup.
pub fn run_gameplay_audio_coordinate_with_practice_and_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
    R: crate::practice_playback::PracticeRecordingPort,
>(
    device: &mut D,
    audio: AudioGameplaySession<'_, S>,
    config: CoordinateGameplayConfig,
    control: &mut C,
    host: &mut H,
    practice: &mut crate::practice_playback::PracticePlayback,
    recording: &mut R,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    run_gameplay_audio_with_practice_and_ports_impl(
        device,
        audio,
        config.audio,
        control,
        host,
        practice,
        recording,
        Some(config.source_history),
    )
}

fn run_gameplay_audio_with_practice_and_ports_impl<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
    R: crate::practice_playback::PracticeRecordingPort,
>(
    device: &mut D,
    audio: AudioGameplaySession<'_, S>,
    config: AudioGameplayConfig,
    control: &mut C,
    host: &mut H,
    practice: &mut crate::practice_playback::PracticePlayback,
    recording: &mut R,
    history: Option<crate::gameplay::output::ports::SourcePlaybackIdentity>,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    let speed = history.and_then(|_| practice.initial_setup());
    if let Some(source_history) = history {
        crate::native_policy_admission::validate_audio_coordinate_start(
            &CoordinateGameplayConfig {
                audio: config,
                source_history,
            },
            audio.session.discipline,
            audio.session.runtime.transport_mut(),
            speed,
        )?;
    } else {
        crate::native_policy_admission::validate_audio_config(&config)?;
    }
    if audio.session.runtime.song_end() != config.gameplay.end_song
        || config.gameplay.end_song != practice.section_end()
    {
        return Err("retained solo Runtime, configuration and practice section ends differ".into());
    }

    let session = audio.session;
    let mut timing = AudioTiming(session.discipline, config.section_start, speed, history);
    let session = GameplaySession {
        runtime: session.runtime,
        gauge: session.gauge,
        bgm: session.bgm,
        discipline: &mut timing,
        pause: session.pause,
        end: session.end,
        completion: session.completion,
        capture: session.capture,
        competition: session.competition,
        delivery: session.delivery,
        pre_origin_inputs: session.pre_origin_inputs,
    };
    let result = run_gameplay_timed(
        device,
        session,
        Some(audio.merger),
        config.gameplay,
        control,
        host,
        Some(RetainedSoloPractice {
            playback: practice,
            recording,
            last_playback_frame: 0,
            completion_template: None,
        }),
    );
    // The native finisher owns the final capture and derives its completed
    // result sidecar from the same evidence. Only retired attempts archive here.
    let revoke = if practice.rate_state().is_some() {
        host.advertise_practice_transactions(None)
    } else {
        host.advertise_practice(None)
    };
    match result {
        Err(error) => Err(error),
        Ok(value) => {
            revoke?;
            Ok(value)
        }
    }
}

fn archive_practice_capture<S, T, R: crate::practice_playback::PracticeRecordingPort>(
    session: &mut GameplaySession<'_, S, T>,
    owner: &mut RetainedSoloPractice<'_, R>,
) -> NativeGameplayResult<()> {
    if let Some(capture) = session.capture.take() {
        let member = &owner.playback.members()[0];
        let path = member
            .launch
            .args()
            .chunks_exact(2)
            .find(|pair| pair[0] == "--record-replay")
            .ok_or("practice capture has no current recording path")?;
        owner
            .recording
            .archive(member.player, std::path::Path::new(&path[1]), capture)?;
    }
    Ok(())
}

#[derive(Default)]
struct SoloCoordinateControls {
    pause: Option<crate::playback_pause::PreparedCoordinatePauseAdoption>,
    end: Option<crate::native_end::PreparedCoordinateEnd>,
    pause_host: Option<ClockPoint>,
    end_host: Option<ClockPoint>,
    end_committed: bool,
}

fn run_gameplay_timed<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
    T: PumpTiming<D>,
    R: crate::practice_playback::PracticeRecordingPort,
>(
    device: &mut D,
    mut session: GameplaySession<'_, S, T>,
    mut merger: Option<&mut InputMerger>,
    mut config: NativeGameplayConfig,
    control: &mut C,
    host_port: &mut H,
    mut practice: Option<RetainedSoloPractice<'_, R>>,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    let speed = session.discipline.live_speed();
    let source_history = session.discipline.source_playback_identity();
    if let Some(source_history) = source_history {
        crate::native_policy_admission::validate_audio_coordinate_start(
            &CoordinateGameplayConfig {
                audio: AudioGameplayConfig {
                    gameplay: config,
                    section_start: session
                        .discipline
                        .section_start()
                        .ok_or("coordinate gameplay lacks section start")?,
                },
                source_history,
            },
            session
                .discipline
                .audio()
                .ok_or("coordinate gameplay lacks audio authority")?,
            session.runtime.transport_mut(),
            speed,
        )?;
    }
    if speed.is_none()
        && session
            .runtime
            .transport_mut()
            .preroll_setup()
            .is_some_and(|setup| setup.positive_rate != beatkernel::transport::Rate::NORMAL)
    {
        return Err("nonnormal live speed requires the explicit original-audio speed owner".into());
    }
    if speed.is_some() && (!T::AUDIO || (practice.is_some() && source_history.is_none())) {
        return Err(
            "selected live speed requires an original-audio owner without retained practice".into(),
        );
    }
    if practice.is_some() && !T::AUDIO {
        return Err("retained practice requires original audio authority".into());
    }
    if let Some(owner) = practice.as_mut() {
        if owner.playback.members().len() != 1
            || owner.playback.members()[0].player != crate::local_players::PlayerId(1)
            || session.runtime.audio_scope() != beatkernel::audio::CommandScope(1)
            || session.end.is_some()
        {
            return Err(
                "retained solo practice requires one installed unlimited scope-one owner".into(),
            );
        }
        if session.runtime.song_end() != config.end_song
            || config.end_song != owner.playback.section_end()
        {
            return Err(
                "retained solo Runtime, configuration and practice section ends differ".into(),
            );
        }
        session
            .discipline
            .audio_mut()
            .expect("audio timing")
            .pin_practice_mixer_basis(owner.playback.mixer_basis())?;
    }
    if T::AUDIO {
        let epoch = session
            .discipline
            .audio()
            .expect("audio timing")
            .authority()
            .epoch();
        if session.runtime.clock_domains()
            != Some((epoch.logical_origin.domain, epoch.stream_origin.domain))
            || epoch.host_domain != config.origin.domain
            || epoch.stream_origin != config.stream_origin
            || session.runtime.transport_mut().anchor().host_time
                != epoch.logical_output(config.playback_origin)?.timestamp
            || session.runtime.transport_mut().anchor().song_time != config.song_origin
            || session.runtime.transport_mut().anchor().rate
                != speed.map_or(beatkernel::transport::Rate::NORMAL, |setup| {
                    setup.speed().rate()
                })
            || session.runtime.poisoned()
            || session.runtime.gameplay_fence().is_some()
        {
            return Err("audio gameplay requires the actual pristine logical/raw Runtime domains and origin".into());
        }
        if speed.is_some() || source_history.is_some() {
            let end = speed.map_or(config.end_song, |setup| setup.end());
            if session.runtime.song_end() != end
                || (practice.is_none() && session.end.is_some() != end.is_some())
                || session.pause.epoch() != epoch.id
                || session.pause.host_domain() != epoch.host_domain
                || session.pause.phase() != PausePhase::Running
            {
                return Err(
                    "selected live speed requires matching original end and pristine pause epoch"
                        .into(),
                );
            }
        }
        if let Some(speed) = speed {
            if source_history.is_none() {
                crate::native_policy_admission::validate_transport_with_speed(
                    session.runtime.transport_mut(),
                    epoch.logical_output(config.playback_origin)?.timestamp,
                    speed,
                )?;
            }
        }
        if merger.as_ref().expect("audio merger").source_capacity() != 4096 {
            return Err("audio solo requires the bounded dynamic actual-source merger".into());
        }
    }
    let mut coordinate_controls = SoloCoordinateControls::default();
    let coordinate_control_required =
        source_history.is_some() && (config.pause_supported || session.end.is_some());
    let coordinate_bindings =
        if let Some(identity) = source_history.filter(|_| coordinate_control_required) {
            let authority = session
                .discipline
                .audio()
                .expect("audio timing")
                .authority();
            let basis = authority
                .coordinate_basis()
                .ok_or("coordinate control lacks descriptor")?;
            let epoch = authority.epoch().id;
            let pause = session
                .pause
                .clone()
                .with_coordinate_binding(epoch, basis, identity)?;
            let end = session
                .end
                .clone()
                .map(|end| end.with_coordinates(epoch, basis, identity))
                .transpose()?;
            Some((pause, end))
        } else {
            None
        };
    let logical_domain = session.discipline.logical_domain(config);
    let section_start = session.discipline.section_start();
    let custom_policy = session.gauge.profile() != &GaugeProfile::default();
    if config.origin.domain == config.stream_origin.domain
        || config.stream_origin.domain != config.playback_origin.domain
        || config.playback_origin.timestamp < config.stream_origin.timestamp
        || config.sample_rate == 0
        || config.sample_rate > 1_000_000_000
        || !(0..=1_000_000_000).contains(&config.advance_lag.as_nanos())
    {
        return Err("invalid native gameplay clocks/rate/lag".into());
    }
    let mut pending = VecDeque::new();
    if custom_policy || speed.is_some() {
        if session.runtime.poisoned() || session.runtime.gameplay_fence().is_some() {
            return Err("nondefault native runtime is poisoned or already fenced".into());
        }
        crate::native_policy_admission::validate_initial(session.runtime.judge(), session.gauge)?;
        if let Some(speed) = speed {
            crate::native_policy_admission::validate_capture_in_domain_with_speed(
                session.runtime.judge(),
                session.gauge.profile(),
                session.capture.as_ref(),
                &config,
                logical_domain,
                section_start,
                speed,
            )?;
        } else {
            crate::native_policy_admission::validate_capture_in_domain(
                session.runtime.judge(),
                session.gauge.profile(),
                session.capture.as_ref(),
                &config,
                logical_domain,
                section_start,
            )?;
        }
        if let Some(competition) = session
            .competition
            .as_ref()
            .filter(|port| !port.policy_agnostic())
        {
            let header = competition
                .expected_policy_header()
                .ok_or("nondefault native competition has no policy identity")?;
            if let Some(speed) = speed {
                crate::native_policy_admission::validate_header_in_domain_with_speed(
                    session.runtime.judge(),
                    session.gauge.profile(),
                    header,
                    &config,
                    logical_domain,
                    section_start,
                    speed,
                )?;
            } else {
                crate::native_policy_admission::validate_header_in_domain(
                    session.runtime.judge(),
                    session.gauge.profile(),
                    header,
                    &config,
                    logical_domain,
                    section_start,
                )?;
            }
            if session
                .capture
                .as_ref()
                .is_some_and(|capture| capture.header() != header)
            {
                return Err("native capture and competition policy identities differ".into());
            }
        }
    }
    pending.try_reserve_exact(4096)?;
    // All cold configuration, capture and competition checks above are read-only.
    // Install the checked owners together immediately before the first host effect.
    if let Some((pause, end)) = coordinate_bindings {
        *session.pause = pause;
        *session.end = end;
    }
    if custom_policy || speed.is_some() {
        host_port
            .prepare_policies(&[(crate::local_players::PlayerId(1), session.gauge.profile())])?;
    }
    let mut deadline =
        NativePumpDeadline::new(control, config.seconds, "native gameplay deadline overflow")?;
    let mut last_acquired = config.origin;
    let mut completed_through = None;
    let mut last_operation = config.origin;
    let mut last_song = config.song_origin;
    let mut pause_song_origin = config.song_origin;
    let mut last_progress = None;
    let mut last_host = None;
    let mut keyboard = PauseKeyboard::new();
    let mut audio_boundary: Option<AudioLivePauseBoundary> = None;
    let mut paused_boundary: Option<LivePauseBoundary> = None;
    let mut resume_boundary: Option<ClockPoint> = None;
    let mut pause_committed = false;
    let mut end_boundary = None;
    let mut end_rendered = false;
    let mut live_terminal = false;
    let mut pause_announced = false;
    let mut stop_evidence = OwnedStopEvidence::default();
    let mut render_evidence = NativeRenderEvidence::default();
    let mut stop_barrier = NativeStopBarrier::default();
    while !host_port.cancelled() && deadline.active(control)? {
        host_port.retry_pause_publication();
        let control_facts = if coordinate_control_required {
            session.discipline.coordinate_playback_control(device)?
        } else {
            session.discipline.observe(device)?;
            None
        };
        if let Some(facts) = control_facts.as_ref() {
            facts.validate(
                session.discipline.audio().expect("audio timing"),
                source_history.expect("coordinate identity"),
            )?;
        }
        let reference = session.discipline.latest_pair();
        if !T::AUDIO && reference.is_none() {
            return Err("gameplay requires native clock relation".into());
        }
        let pause_now = if T::AUDIO {
            Some(device.host_now()?)
        } else {
            None
        };
        let rendered = render_evidence.validate(
            if coordinate_control_required {
                control_facts.as_ref().and_then(|facts| facts.report())
            } else {
                device.render_report()?
            },
            &stop_evidence,
        )?;
        if T::AUDIO && session.end.is_some() {
            if let Some(report) = rendered
                .filter(|report| report.frames != 0 && report.playback_end_physical_frame.is_some())
            {
                // This validates the immutable finite source grid and marker,
                // including startup and pause gaps. It grants no native target
                // crossing or command ACK; those completion gates remain below.
                session.pause.scheduling_point(report)?;
                session.runtime.seal_gameplay_audio();
            }
        }
        if source_history.is_none() {
            if let Some(end) = session.end.as_mut() {
                end_rendered |=
                    rendered.is_some_and(|report| report.playback_end_physical_frame.is_some());
                if let Some(boundary) = session.discipline.end(device, end, rendered)? {
                    end_boundary = Some(boundary);
                }
            }
        }
        if source_history.is_some() {
            if let Some(facts) = control_facts {
                let audio = session.discipline.audio().expect("audio timing");
                if let Some(end) = session
                    .end
                    .as_mut()
                    .filter(|_| !coordinate_controls.end_committed)
                {
                    let stage_end = |end: &NativeEnd| {
                        match facts {
                        crate::native_coordinate_control::CoordinatePlaybackControlFacts::Source(_) => {
                            end.stage_coordinate_source(
                                facts.epoch(),
                                facts.basis(),
                                facts.identity(),
                                rendered,
                                facts.pair(),
                            )
                        }
                        crate::native_coordinate_control::CoordinatePlaybackControlFacts::Target(target) => {
                            end.stage_coordinate_target(
                                facts.epoch(),
                                facts.basis(),
                                facts.identity(),
                                target.target().boundaries,
                                rendered,
                                facts.pair(),
                            )
                        }
                    }
                    };
                    let staged = stage_end(end)?;
                    end.commit_coordinate_observation(staged)?;
                    let staged = stage_end(end)?;
                    end_rendered |= staged.query().is_some();
                    coordinate_controls.end = Some(staged);
                }
                if config.pause_supported && !pause_announced {
                    pause_announced = true;
                    host_port.publish_pause(PauseState::Running);
                }
                if config.pause_supported
                    && !end_rendered
                    && (session.pause.phase() == PausePhase::Running || pause_committed)
                    && coordinate_controls.pause.is_none()
                {
                    let desired = host_port.pause_requested();
                    if (desired || !device.output_replacement_pending())
                        && session
                            .pause
                            .request_coordinate(desired, audio.authority())?
                    {
                        session.runtime.request_audio_pause(desired);
                        if !desired
                            && matches!(
                                facts.basis(),
                                crate::audio_authority::AudioAuthorityCoordinateBasis::TargetFrame(
                                    _
                                )
                            )
                        {
                            device.set_audio_held(false)?;
                        }
                        host_port.publish_pause(if desired {
                            PauseState::Pausing
                        } else {
                            PauseState::Resuming
                        });
                    }
                }
                if coordinate_controls.pause.is_none() {
                    coordinate_controls.pause = session
                        .pause
                        .prepare_coordinate_adoption(rendered, facts.target_boundaries())?;
                }
            }
            let authority = session
                .discipline
                .audio()
                .expect("audio timing")
                .authority();
            let now = pause_now.expect("audio HOST");
            coordinate_controls.pause_host = coordinate_controls
                .pause
                .as_ref()
                .map(|staged| {
                    authority.presented_coordinate_boundary_host(&staged.exact_cut(), now)
                })
                .transpose()?
                .flatten();
            coordinate_controls.end_host = coordinate_controls
                .end
                .as_ref()
                .and_then(|staged| staged.query())
                .map(|query| authority.presented_coordinate_boundary_host(&query, now))
                .transpose()?
                .flatten();
        }
        if source_history.is_some()
            && session.discipline.audio().is_some_and(|audio| {
                matches!(
                    audio.authority().coordinate_basis(),
                    Some(
                        crate::audio_authority::AudioAuthorityCoordinateBasis::TargetFrame(_)
                            | crate::audio_authority::AudioAuthorityCoordinateBasis::SourceFrame(_)
                            | crate::audio_authority::AudioAuthorityCoordinateBasis::SourceCounter { .. }
                    )
                )
            })
        {
            let (queued_source, queued_target, held_source, held_target) = if let Some(owner) =
                practice.as_mut()
            {
                // Capture queue pins before transferring its first receipt
                // into the app owner. The later service peek is idempotent.
                let (queued_source, queued_target) = owner.playback.queued_history_retention_pins();
                NativePracticeBoundary::pending(owner.playback)?;
                let held = owner.playback.history_retention_pins();
                (
                    queued_source,
                    queued_target,
                    held.map(|pins| pins.0),
                    held.map(|pins| pins.1),
                )
            } else {
                (None, None, None, None)
            };
            let source_minimum = match (queued_source, held_source) {
                (Some(queued), Some(held)) => Some(queued.min(held)),
                (queued, held) => queued.or(held),
            };
            let queries = [
                session.pause.pending_coordinate_retention_query()?,
                coordinate_controls
                    .pause
                    .as_ref()
                    .map(|staged| staged.exact_cut()),
                coordinate_controls
                    .end
                    .as_ref()
                    .and_then(|staged| staged.query()),
                queued_target,
                held_target,
            ];
            session
                .discipline
                .retire_coordinate_history(device, &queries, source_minimum)?;
        }
        if source_history.is_none()
            && (config.logical_schedule || config.pause_supported)
            && reference.is_some()
        {
            let desired = (config.pause_supported
                && !end_rendered
                && (session.pause.phase() == PausePhase::Running || pause_committed)
                && resume_boundary.is_none())
            .then(|| host_port.pause_requested())
            .filter(|desired| *desired || !device.output_replacement_pending());
            let reference = reference.expect("pause has accepted reference");
            let evidence = session.discipline.pause_evidence(
                device,
                reference,
                pause_now.unwrap_or(reference.target),
            )?;
            let target_pause = matches!(
                evidence,
                crate::live_pause::LivePauseObservation::Target { .. }
            );
            if target_pause && !T::AUDIO {
                return Err("target pause requires original audio presentation".into());
            }
            let mut staged_pause = target_pause.then(|| session.pause.clone());
            let pause = staged_pause.as_mut().unwrap_or(&mut *session.pause);
            let mut staged_audio_boundary = None;
            let update = if T::AUDIO {
                let update = if let Some(speed) = speed {
                    crate::live_pause::update_live_audio_pause_with_speed(
                        pause,
                        evidence,
                        rendered,
                        desired,
                        speed,
                        config.sample_rate,
                    )?
                } else {
                    update_live_audio_pause(
                        pause,
                        evidence,
                        rendered,
                        desired,
                        pause_song_origin,
                        config.sample_rate,
                    )?
                };
                if let Some(boundary) = update.boundary {
                    staged_audio_boundary = Some(boundary);
                }
                crate::live_pause::LivePauseUpdate {
                    requested: update.requested,
                    boundary: update.boundary.map(|value| value.original),
                    observed: update.observed,
                }
            } else {
                update_live_pause(
                    pause,
                    evidence,
                    rendered,
                    desired,
                    config.song_origin,
                    config.sample_rate,
                )?
            };
            if target_pause {
                if update.requested == Some(false) {
                    device.set_audio_held(false)?;
                } else if update.boundary.is_some_and(|boundary| boundary.paused) {
                    device.set_audio_held(true)?;
                }
            }
            if let Some(pause) = staged_pause {
                *session.pause = pause;
            }
            if let Some(boundary) = staged_audio_boundary {
                audio_boundary = Some(boundary);
            }
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
                if T::AUDIO {
                    if boundary.paused && !end_rendered {
                        paused_boundary = Some(boundary);
                        pause_committed = false;
                    }
                } else if boundary.paused {
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
                    if !T::AUDIO {
                        let transport = prepare_live_transport(
                            session.runtime.transport_mut(),
                            boundary,
                            last_song,
                        )?;
                        session
                            .discipline
                            .resume(device, config, session.pause, reference)?;
                        *session.runtime.transport_mut() = transport;
                    }
                    resume_boundary = Some(boundary.at);
                    paused_boundary = None;
                    pause_committed = false;
                }
            }
        }
        if practice.is_none() {
            let rendered = render_evidence.validate(device.render_report()?, &stop_evidence)?;
            crate::native_audio::feed_rendered(session.bgm, rendered, |command| {
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
        let frozen_pause = paused_boundary
            .filter(|_| {
                pause_committed && session.pause.phase() == PausePhase::Paused && !end_rendered
            })
            .map(|boundary| boundary.at);
        crate::gameplay_presentation::validate_pump_host(
            device,
            session.discipline,
            received,
            frozen_pause,
        )?;
        validate_audio_availability::<D, T>(device, session.discipline, received, config.origin)?;
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
        crate::gameplay_presentation::validate_pump_host(
            device,
            session.discipline,
            received,
            frozen_pause,
        )?;
        let cut = completed_cut(
            batch.completed_through,
            received,
            config.origin,
            &mut completed_through,
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
            if !T::AUDIO {
                chronology(host, last_acquired)?;
                last_acquired = host;
            }
            crate::gameplay_presentation::validate_pump_host(
                device,
                session.discipline,
                host,
                frozen_pause,
            )?;
            session.delivery.observe(host, received)?;
            index += 1;
        }
        if T::AUDIO {
            let input_merger = merger.as_deref_mut().expect("audio merger");
            while let Some(event) = pending.pop_front() {
                input_merger.register_source(event.meta().source)?;
                input_merger.admit(event, received)?;
            }
            if let Some(prefix) = cut
                .map(|cut| {
                    input_merger.watermark(cut, config.advance_lag.as_nanos(), batch.backlog)
                })
                .transpose()?
                .flatten()
            {
                session
                    .discipline
                    .audio_mut()
                    .expect("audio timing")
                    .authority_mut()
                    .record_acquired_prefix(prefix)?;
            }
            if source_history.is_some()
                && pause_committed
                && session.pause.phase() == PausePhase::Paused
                && !end_rendered
            {
                service_paused_coordinate_solo(
                    device,
                    &mut session,
                    &mut config,
                    input_merger,
                    received,
                    &mut keyboard,
                    &mut last_operation,
                    end_boundary,
                    host_port,
                    practice
                        .as_ref()
                        .is_some_and(|owner| owner.playback.transition_pending()),
                )?;
            }
        }
        if source_history.is_none()
            && (config.logical_schedule || config.pause_supported)
            && (session.pause.last_render_report().is_none()
                || matches!(
                    session.pause.phase(),
                    PausePhase::Pausing | PausePhase::Resuming
                ))
        {
            control.wait(WallDuration::from_millis(1))?;
            continue;
        }
        if T::AUDIO {
            let input_merger = merger.as_deref_mut().expect("audio merger");
            if let Some(owner) = practice.as_mut() {
                // Receipts fence input and advancement before any ordinary pump
                // work. A render cursor alone never authorizes an input cut.
                let mut waiting = false;
                while NativePracticeBoundary::pending(owner.playback)? {
                    match service_practice_solo(
                        device,
                        &mut session,
                        &mut config,
                        input_merger,
                        received,
                        batch.backlog,
                        &mut keyboard,
                        &mut paused_boundary,
                        &mut audio_boundary,
                        &mut pause_committed,
                        &mut last_song,
                        &mut last_operation,
                        &mut stop_evidence,
                        host_port,
                        owner,
                        &mut render_evidence,
                    )? {
                        PracticeService::Waiting => {
                            waiting = true;
                            break;
                        }
                        PracticeService::Committed { fresh, terminal } => {
                            if fresh {
                                keyboard = PauseKeyboard::new();
                                paused_boundary = None;
                                audio_boundary = None;
                                resume_boundary = None;
                                pause_committed = false;
                                end_boundary = None;
                                end_rendered = false;
                                // Stop evidence shares the retained Mixer's
                                // cumulative lifetime; only the attempt's
                                // completion barrier starts over here.
                                stop_barrier = NativeStopBarrier::default();
                                last_progress = None;
                                // Pause's cursor is cumulative on the same Mixer.
                                // The song-grid origin is distinct from the new
                                // attempt's requested start used in results.
                                pause_song_origin = Timestamp::from_nanos(i64::try_from(
                                    i128::from(config.song_origin.as_nanos())
                                        - i128::from(owner.last_playback_frame) * 1_000_000_000
                                            / i128::from(config.sample_rate),
                                )?);
                                host_port.publish_pause(PauseState::Running);
                            }
                            if terminal {
                                return complete_gameplay(&mut session, config, host_port);
                            }
                        }
                    }
                }
                if waiting {
                    control.wait(WallDuration::from_millis(1))?;
                    continue;
                }
                if owner.playback.rate_state().is_some() {
                    let fixed_competition = session
                        .competition
                        .as_ref()
                        .is_some_and(|competition| !competition.policy_agnostic());
                    let mut captures = [(crate::local_players::PlayerId(1), &mut *session.capture)];
                    owner.playback.service_transaction(
                        host_port,
                        session.pause.phase() == PausePhase::Running,
                        session.runtime.transport_mut(),
                        &mut captures,
                        fixed_competition,
                    )?;
                } else if session.pause.phase() == PausePhase::Running {
                    if let Some(report) =
                        render_evidence.validate(device.render_report()?, &stop_evidence)?
                    {
                        owner.playback.service_request(
                            host_port,
                            report
                                .playback_start_frame
                                .checked_add(u64::try_from(report.playback_frames)?)
                                .ok_or("practice playback cursor overflow")?,
                        )?;
                    }
                }
            }
            let practice_end = practice.as_ref().and_then(|_| config.end_song);
            let practice_source_cut = practice.as_ref().and_then(|owner| {
                NativePracticeBoundary::unprojected_source_cut(owner.playback)
            });
            service_audio_solo(
                device,
                &mut session,
                &mut config,
                input_merger,
                received,
                batch.backlog,
                &mut keyboard,
                &mut paused_boundary,
                &mut audio_boundary,
                &mut pause_committed,
                &mut last_song,
                &mut last_operation,
                end_rendered,
                end_boundary,
                &mut stop_evidence,
                host_port,
                None,
                practice_source_cut,
                practice_end,
                &mut render_evidence,
                Some(&mut coordinate_controls),
                &mut end_boundary,
                &mut live_terminal,
            )?;
            if (session.pause.phase() == PausePhase::Paused && !end_rendered)
                || audio_boundary.is_some()
                || matches!(
                    session.pause.phase(),
                    PausePhase::Pausing | PausePhase::Resuming
                )
            {
                control.wait(WallDuration::from_millis(1))?;
                continue;
            }
        } else {
            if let Some(at) = resume_boundary {
                while pending
                    .front()
                    .is_some_and(|event| point(event).timestamp < at.timestamp)
                {
                    keyboard.observe_paused(pending.pop_front().unwrap())?;
                }
                if batch.backlog || cut.is_none_or(|cut| cut.timestamp < at.timestamp) {
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
                        &mut render_evidence,
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
                    &mut render_evidence,
                )?;
                last_operation = host;
            }
            if !batch.backlog {
                if let Some(boundary) = paused_boundary.filter(|_| !pause_committed) {
                    let at = boundary.at;
                    if cut.is_some_and(|cut| cut.timestamp >= at.timestamp) {
                        chronology(at, last_operation)?;
                        let audio_at = schedule(
                            device,
                            &mut session,
                            config,
                            &mut render_evidence,
                            &stop_evidence,
                        )?;
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
                session.discipline.publish(
                    device,
                    crate::gameplay_presentation::GameplayPauseControl::solo(session.runtime),
                    None,
                    session.pause,
                    &mut config,
                    session.end,
                    received,
                    false,
                )?;
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
            if let Some(DisciplineUpdate::Applied {
                base_rate_ppm,
                correction_ppm,
                applied_rate_ppm,
                phase_error_ns,
                limited,
            }) = session
                .discipline
                .correction(now, session.runtime.transport_mut())?
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
            if let Some(at) = cut
                .map(|cut| {
                    watermark(
                        config.origin,
                        last_operation,
                        cut,
                        config.advance_lag,
                        batch.backlog,
                    )
                })
                .transpose()?
                .flatten()
            {
                let audio_at = schedule(
                    device,
                    &mut session,
                    config,
                    &mut render_evidence,
                    &stop_evidence,
                )?;
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
        }
        // Read after this iteration's admissions. A pre-admission idle block
        // cannot establish that newly queued Stops reached the mixer.
        let rendered = render_evidence.validate(device.render_report()?, &stop_evidence)?;
        let numeric_terminal = session.gauge.snapshot().failure.is_some()
            && session.runtime.gameplay_fence().is_some();
        let stops_rendered = if config.end_song.is_some() {
            stop_barrier.observe(&stop_evidence, rendered)?
        } else {
            true
        };
        let acquired_ready = cut.is_some_and(|cut| {
            !batch.backlog
                && i128::from(cut.timestamp.as_nanos()) - i128::from(config.advance_lag.as_nanos())
                    >= i128::from(last_operation.timestamp.as_nanos())
        });
        if let Some(speed) = speed.filter(|_| !live_terminal && source_history.is_none()) {
            if let (Some(end), Some(boundary)) = (speed.end(), end_boundary) {
                if speed.playback_end_frame(config.sample_rate)? != Some(boundary.playback_frame) {
                    return Err(
                        "selected live speed differs from actual finite playback endpoint".into(),
                    );
                }
                let authority = session
                    .discipline
                    .audio()
                    .expect("selected audio timing")
                    .authority();
                let input_drained = authority
                    .acquired_prefix()
                    .is_some_and(|prefix| authority.closed_host_prefix() == Some(prefix));
                if stops_rendered
                    && acquired_ready
                    && input_drained
                    && pending.is_empty()
                    && merger.as_ref().is_some_and(|merger| merger.pending() == 0)
                    && audio_boundary.is_none()
                    && resume_boundary.is_none()
                    && finite_frontier_ready(
                        true,
                        Some(boundary),
                        last_operation,
                        batch.backlog,
                        false,
                    )
                    && finite_terminal_output_ready(
                        session.bgm.report(),
                        rendered,
                        session.runtime.admitted_audio_commands(),
                    )
                {
                    if source_history.is_some() {
                        return Err(
                            "coordinate finite completion requires an exact native end control"
                                .into(),
                        );
                    }
                    // This is the last real committed operation, or the actual
                    // cold logical origin for a genuine zero-frame endpoint.
                    let frontier = authority
                        .committed_operation()
                        .unwrap_or(authority.epoch().logical_output(config.playback_origin)?);
                    session
                        .runtime
                        .transport_mut()
                        .seek(frontier.timestamp, end)?;
                    let audio_at = schedule(
                        device,
                        &mut session,
                        config,
                        &mut render_evidence,
                        &stop_evidence,
                    )?;
                    let report =
                        session
                            .runtime
                            .advance_to(frontier, &ExplicitDomains, audio_at)?;
                    last_song = report.song_time;
                    publish_with_host(&mut session, report, &mut stop_evidence, host_port)?;
                    live_terminal = true;
                }
            }
        }
        if practice.is_none()
            && (speed.is_none()
                || config.end_song.is_none()
                || (live_terminal
                    && finite_terminal_output_ready(
                        session.bgm.report(),
                        rendered,
                        session.runtime.admitted_audio_commands(),
                    )))
            && stops_rendered
            && acquired_ready
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
        let natural_practice_ready = practice.as_ref().is_none_or(|owner| {
            owner.playback.can_complete_naturally()
                && last_song >= owner.playback.natural_completion_floor()
        });
        if natural_practice_ready
            && config.end_song.is_none()
            && acquired_ready
            && !batch.backlog
            && pending.is_empty()
            && merger.as_ref().is_none_or(|value| value.pending() == 0)
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
    mod policy_admission {
        use super::*;
        include!("native_policy_admission_fixtures.rs");
    }
    include!("native_gameplay_interval_fixtures.rs");
    include!("native_input_gameplay_fixtures.rs");
    use super::*;
    use beatkernel::{
        audio::*,
        input::{
            Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
            EventMeta, GameControlId, PhysicalControlId,
        },
        judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
        replay::{codec::ReplayCodecLimits, ReplayOperation},
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
                    completed_through: Some(point(1, self.step as i64 * 10_000_000)),
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
                    completed_through: Some(point(1, self.step as i64 * 10_000_000)),
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
            assert!(capture
                .records()
                .iter()
                .any(|record| matches!(record.operation, ReplayOperation::Input(_))));
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
        assert!(watermark(
            point(1, 30),
            point(1, 30),
            point(1, 20),
            Duration::ZERO,
            false
        )
        .unwrap()
        .is_none());
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

pub(crate) fn validate_audio_availability<
    D: crate::gameplay_presentation::GameplayDevice,
    T: PumpTiming<D>,
>(
    device: &D,
    timing: &T,
    now: ClockPoint,
    origin: ClockPoint,
) -> NativeGameplayResult<()> {
    let Some(presentation) = timing.audio() else {
        return Ok(());
    };
    if device.output_clock_suspended() {
        return Ok(());
    }
    if !presentation.authority().has_fresh_observation(now)?
        && i128::from(now.timestamp.as_nanos()) - i128::from(origin.timestamp.as_nanos())
            > i128::from(
                presentation
                    .authority()
                    .config()
                    .max_observation_age
                    .as_nanos(),
            )
    {
        return Err("native audio observation is unavailable beyond its declared age bound".into());
    }
    Ok(())
}

/// Reconciliation maps only the authorized control occurrence, not a new physical input clock.
pub(crate) struct AudioControlMapper(pub crate::audio_authority::PreparedControlCutoff);
impl ClockMapper for AudioControlMapper {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from == self.0.host() && to == self.0.output().domain).then_some(self.0.output().timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}

/// Qualify the earliest render receipt before draining any original input.
/// Later receipts remain in the bounded queue until this exact cut commits.
fn service_practice_solo<
    D: crate::gameplay_presentation::GameplayDevice,
    S: SoloCompetitionPort,
    H: NativeGameplayHost,
    T: PumpTiming<D>,
    R: crate::practice_playback::PracticeRecordingPort,
>(
    device: &mut D,
    session: &mut GameplaySession<'_, S, T>,
    config: &mut NativeGameplayConfig,
    merger: &mut InputMerger,
    now: ClockPoint,
    backlog: bool,
    keyboard: &mut PauseKeyboard,
    paused: &mut Option<LivePauseBoundary>,
    transition: &mut Option<AudioLivePauseBoundary>,
    pause_committed: &mut bool,
    last_song: &mut Timestamp,
    last_host_operation: &mut ClockPoint,
    stops: &mut OwnedStopEvidence,
    host: &mut H,
    owner: &mut RetainedSoloPractice<'_, R>,
    render_evidence: &mut NativeRenderEvidence,
) -> NativeGameplayResult<PracticeService> {
    use beatkernel::audio::{CommandScope, PracticeBoundaryKind};
    let Some(boundary) = NativePracticeBoundary::qualify(
        owner.playback,
        device,
        session.discipline.audio().expect("audio timing"),
        session.discipline.source_playback_identity(),
        session.runtime.transport_mut().output_phase_basis(),
        now,
    )?
    else {
        return Ok(PracticeService::Waiting);
    };
    let coordinate = if let Some(query) = boundary
        .coordinate_query()?
        .filter(|_| !matches!(boundary, NativePracticeBoundary::RuntimeRate(_)))
    {
        let identity = session
            .discipline
            .source_playback_identity()
            .ok_or("coordinate practice lacks original source identity")?;
        let Some(mapping) = device.conditional_playback_mapping_at(
            session.discipline.audio().expect("audio timing"),
            &query,
            identity,
        )?
        else {
            return Ok(PracticeService::Waiting);
        };
        let basis = session
            .runtime
            .transport_mut()
            .output_phase_basis()
            .ok_or("coordinate practice lacks output phase")?;
        if basis.source_hz() != identity.hz() {
            return Err("coordinate practice source grid differs from output phase".into());
        }
        let phase = if boundary
            .receipt()
            .is_none_or(|receipt| receipt.kind != PracticeBoundaryKind::Started)
        {
            let phase = mapping.phase(
                session.discipline.audio().expect("audio timing"),
                identity,
                &query,
                basis,
                boundary.output(),
            )?;
            session
                .runtime
                .transport_mut()
                .position_at_output_phase(phase)?;
            Some(phase)
        } else {
            None
        };
        Some((basis, phase, mapping, identity, query))
    } else {
        None
    };
    service_audio_solo(
        device,
        session,
        config,
        merger,
        now,
        backlog,
        keyboard,
        paused,
        transition,
        pause_committed,
        last_song,
        last_host_operation,
        false,
        None,
        stops,
        host,
        Some(boundary.host()),
        None,
        None,
        render_evidence,
        None,
        &mut None,
        &mut false,
    )?;
    if backlog
        || !boundary.ready(session.discipline.audio().expect("audio timing"), now, merger)?
    {
        return Ok(PracticeService::Waiting);
    }
    if session.pause.phase() != PausePhase::Running || transition.is_some() || paused.is_some() {
        return Err("practice boundary conflicts with an actual pause transition".into());
    }
    if let NativePracticeBoundary::RuntimeRate(cut) = boundary {
        let phase = cut.phase();
        let audio_at = schedule(device, session, *config, render_evidence, stops)?;
        let report = session.runtime.advance_to_output_phase(
            boundary.output(),
            &ExplicitDomains,
            audio_at,
            phase,
        )?;
        *last_song = report.song_time;
        *last_host_operation = boundary.host();
        publish_with_host(session, report, stops, host)?;
        let mut captures = [(crate::local_players::PlayerId(1), &mut *session.capture)];
        let prepared = owner.playback.prepare_rate_event_at_cut(
            &cut,
            session.runtime.transport_mut(),
            &mut captures,
        )?;
        boundary.commit(session.discipline.audio_mut().expect("audio timing"), now, merger)?;
        let applied = owner.playback.apply_rate_event(prepared)?;
        owner.playback.publish_rate_event(&applied, host)?;
        return Ok(PracticeService::Committed {
            fresh: false,
            terminal: owner.playback.terminal_ready(),
        });
    }
    let mut prepared = boundary.prepare(owner.playback, host)?;
    let receipt = prepared.receipt;
    let fresh = matches!(
        receipt.kind,
        PracticeBoundaryKind::Requested | PracticeBoundaryKind::Looped
    );
    let fresh_transport = if fresh {
        if let Some((old, _, mapping, identity, query)) = coordinate {
            if prepared.attempts.len() != 1 {
                return Err("practice payload differs from solo cohort".into());
            }
            let attempt = &prepared.attempts[0].1;
            if receipt.generation == 0 || receipt.generation <= session.runtime.audio_scope().0 {
                return Err("practice audio scope must advance explicitly".into());
            }
            let current = session.runtime.transport_mut();
            if receipt.generation <= old.generation()
                || receipt.playback_frame < old.active_origin_frame()
                || boundary.output().timestamp < current.anchor().host_time
                || (owner.playback.rate_state().is_none()
                    && attempt.speed().rate() != current.anchor().rate)
                || attempt.config.start != receipt.applied_song_time
            {
                return Err("practice attempt differs from retained output phase".into());
            }
            let basis = beatkernel::transport::OutputPhaseBasis::new(
                old.domain(),
                old.owner(),
                receipt.generation,
                receipt.playback_frame,
                old.source_hz(),
            )?;
            let transport = if owner.playback.rate_state().is_some() {
                attempt.transport_at_runtime_output_phase(basis, boundary.output())?
            } else {
                attempt.transport_at_output_phase(basis, boundary.output())?
            };
            let phase = mapping.phase(
                session.discipline.audio().expect("audio timing"),
                identity,
                &query,
                basis,
                boundary.output(),
            )?;
            transport.position_at_output_phase(phase)?;
            Some(transport)
        } else {
            Some(beatkernel::transport::Transport::new(
                boundary.output().timestamp,
                receipt.applied_song_time,
                beatkernel::transport::Rate::NORMAL,
            ))
        }
    } else {
        None
    };
    let competition = if fresh {
        let attempt = &prepared
            .attempts
            .first()
            .ok_or("missing solo practice attempt")?
            .1;
        session
            .competition
            .as_ref()
            .map(|port| port.prepare_practice(attempt, &owner.playback.members()[0].policy))
            .transpose()?
    } else {
        None
    };
    let completion = if fresh {
        let attempt = &prepared
            .attempts
            .first()
            .ok_or("missing solo practice attempt")?
            .1;
        if attempt.config.end.is_none() {
            Some(
                owner
                    .completion_template
                    .as_ref()
                    .or(session.completion.as_ref())
                    .ok_or("full-song practice lacks original completion preparation")?
                    .prepare_practice(&attempt.source, &attempt.judge)?,
            )
        } else {
            None
        }
    } else {
        None
    };
    let mut presentation = if fresh {
        Some(
            prepared
                .presentation
                .take()
                .ok_or("missing practice UI state")?,
        )
    } else {
        None
    };
    // A genuine control occurrence advances the retired attempt to the cut.
    // Started is bootstrap evidence for the already installed initial state.
    if receipt.kind != PracticeBoundaryKind::Started {
        let audio_at = schedule(device, session, *config, render_evidence, stops)?;
        let report = if let Some((_, phase, _, _, _)) = coordinate {
            session.runtime.advance_to_output_phase(
                boundary.output(),
                &ExplicitDomains,
                audio_at,
                phase.expect("nonbootstrap practice phase"),
            )?
        } else {
            session
                .runtime
                .advance_to(boundary.output(), &ExplicitDomains, audio_at)?
        };
        *last_song = report.song_time;
        *last_host_operation = boundary.host();
        publish_with_host(session, report, stops, host)?;
    }
    if fresh {
        if prepared.attempts.len() != 1 {
            return Err("practice payload differs from solo cohort".into());
        }
        archive_practice_capture(session, owner)?;
        let (_, attempt) = prepared.attempts.pop().expect("prepared solo attempt");
        let transport = fresh_transport.expect("prepared fresh transport");
        if boundary.restarts_original() {
            session.runtime.replace_practice_state_with_runtime_restart(
                attempt.judge,
                transport,
                attempt.config.end,
                CommandScope(receipt.generation),
                owner.playback.speed().rate(),
            )?;
        } else if coordinate.is_some() {
            session.runtime.replace_practice_state_with_output_phase(
                attempt.judge,
                transport,
                attempt.config.end,
                CommandScope(receipt.generation),
            )?;
        } else {
            session.runtime.replace_practice_state(
                attempt.judge,
                transport,
                attempt.config.end,
                CommandScope(receipt.generation),
            )?;
        }
        *session.gauge = attempt.gauge;
        *session.capture = attempt.capture;
        *session.competition = competition;
        *session.end = None;
        if owner.completion_template.is_none() {
            owner.completion_template = session.completion.take();
        }
        *session.completion = completion;
        config.song_origin = receipt.applied_song_time;
        config.end_song = attempt.config.end;
        session.discipline.set_section_start(attempt.config.start);
        *last_song = receipt.applied_song_time;
        *last_host_operation = boundary.host();
        owner.last_playback_frame = receipt.playback_frame;
    }
    boundary.commit(session.discipline.audio_mut().expect("audio timing"), now, merger)?;
    // Applied attempt identity is authoritative before fallible publication.
    // In particular, final cleanup must archive the installed fresh capture
    // under its new ordinal even when a screen observer refuses or cancels.
    let applied = owner.playback.apply_boundary(&mut prepared)?;
    if let Some(mut presentation) = presentation.take() {
        host.apply_practice_identity(&mut presentation, receipt.generation);
        host.commit_practice_presentation(presentation, receipt.generation)?;
    }
    owner.playback.publish_boundary(&applied, host)?;
    Ok(PracticeService::Committed {
        fresh,
        terminal: owner.playback.terminal_ready(),
    })
}

/// Maps only keyboard reconciliation at the already qualified original control cut.
struct CoordinateControlMapper(crate::audio_authority::PreparedCoordinateControlCutoff);
impl ClockMapper for CoordinateControlMapper {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from == self.0.host() && to == self.0.output().domain).then_some(self.0.output().timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}

fn service_coordinate_solo_control<
    D: crate::gameplay_presentation::GameplayDevice,
    S: SoloCompetitionPort,
    H: NativeGameplayHost,
    T: PumpTiming<D>,
>(
    device: &mut D,
    session: &mut GameplaySession<'_, S, T>,
    config: &NativeGameplayConfig,
    merger: &mut InputMerger,
    now: ClockPoint,
    backlog: bool,
    keyboard: &mut PauseKeyboard,
    paused: &mut Option<LivePauseBoundary>,
    pause_committed: &mut bool,
    last_song: &mut Timestamp,
    last_host: &mut ClockPoint,
    controls: &mut SoloCoordinateControls,
    end_boundary: &mut Option<EndBoundary>,
    terminal: &mut bool,
    stops: &mut OwnedStopEvidence,
    host: &mut H,
    render_evidence: &mut NativeRenderEvidence,
) -> NativeGameplayResult<()> {
    if !backlog {
        if let (Some(adoption), Some(at)) = (controls.pause.as_ref(), controls.pause_host) {
            let authority = session
                .discipline
                .audio()
                .expect("audio timing")
                .authority();
            let prepared = if adoption.paused() {
                authority.prepare_coordinate_control_cutoff(
                    adoption.epoch(),
                    adoption.exact_cut(),
                    at,
                    now,
                    merger,
                )?
            } else {
                authority.prepare_coordinate_resume_control_cutoff(
                    adoption.epoch(),
                    adoption.exact_cut(),
                    at,
                    now,
                    merger,
                )?
            };
            if let Some(prepared) = prepared {
                session
                    .pause
                    .validate_coordinate_adoption(adoption, authority, &prepared, now, merger)?;
                let basis = session
                    .runtime
                    .transport_mut()
                    .output_phase_basis()
                    .ok_or("coordinate pause lacks phase")?;
                let transport = crate::live_pause::prepare_coordinate_live_transport(
                    session.runtime.transport_mut(),
                    adoption,
                    &prepared,
                    basis,
                    *last_song,
                )?;
                let frames = adoption
                    .playback_frame()
                    .checked_sub(basis.active_origin_frame())
                    .ok_or("coordinate adoption precedes active origin")?;
                let phase = basis.point_from_elapsed_nanos(
                    prepared.output(),
                    i128::from(frames) * 1_000_000_000 / i128::from(basis.source_hz()),
                )?;
                let audio_at = schedule(device, session, *config, render_evidence, stops)?;
                let prior_transport = session.runtime.transport_mut().clone();
                *session.runtime.transport_mut() = transport;
                let report = match session.runtime.advance_to_output_phase(
                    prepared.output(),
                    &ExplicitDomains,
                    audio_at,
                    phase,
                ) {
                    Ok(report) => report,
                    Err(error) => {
                        *session.runtime.transport_mut() = prior_transport;
                        return Err(error.into());
                    }
                };
                let adoption = controls.pause.take().expect("preflighted adoption");
                let boundary = session.pause.commit_coordinate_adoption(
                    adoption,
                    session
                        .discipline
                        .audio()
                        .expect("audio timing")
                        .authority(),
                    &prepared,
                    now,
                    merger,
                )?;
                session
                    .discipline
                    .audio_mut()
                    .expect("audio timing")
                    .authority_mut()
                    .commit_coordinate_control_cutoff(prepared, now, merger)?;
                *last_song = report.song_time;
                *last_host = boundary.host;
                controls.pause_host = None;
                if boundary.paused {
                    *paused = Some(LivePauseBoundary {
                        paused: true,
                        window: crate::native_start::HostStartWindow::new(
                            boundary.host,
                            boundary.host,
                        )?,
                        at: boundary.host,
                        playback_frame: boundary.playback_frame,
                        song: *last_song,
                    });
                    *pause_committed = true;
                } else {
                    *paused = None;
                    *pause_committed = false;
                }
                let publication = publish_with_host(session, report, stops, host);
                let held = if boundary.paused
                    && matches!(
                        session
                            .discipline
                            .audio()
                            .expect("audio timing")
                            .authority()
                            .coordinate_basis(),
                        Some(crate::audio_authority::AudioAuthorityCoordinateBasis::TargetFrame(_))
                    ) {
                    device.set_audio_held(true)
                } else {
                    Ok(())
                };
                publication?;
                held?;
                if !boundary.paused {
                    for event in keyboard.resume(boundary.host)? {
                        let prepared = session
                            .discipline
                            .audio()
                            .expect("audio timing")
                            .authority()
                            .prepare_coordinate_resume_control_cutoff(
                                boundary.epoch,
                                boundary.exact_cut,
                                boundary.host,
                                now,
                                merger,
                            )?
                            .ok_or("coordinate reconciliation lost exact cutoff")?;
                        let phase = basis
                            .point_from_elapsed_nanos(prepared.output(), phase.elapsed_nanos())?;
                        let report = session.runtime.process_input_output_phase(
                            event,
                            &CoordinateControlMapper(prepared),
                            audio_at,
                            phase,
                        )?;
                        session
                            .discipline
                            .audio_mut()
                            .expect("audio timing")
                            .authority_mut()
                            .commit_coordinate_control_cutoff(prepared, now, merger)?;
                        *last_song = report.song_time;
                        publish_with_host(session, report, stops, host)?;
                    }
                }
                host.publish_pause(if boundary.paused {
                    PauseState::Paused
                } else {
                    PauseState::Running
                });
            }
        }
        if let (Some(staged), Some(at)) = (controls.end.as_ref(), controls.end_host) {
            if let Some(query) = staged.query() {
                let authority = session
                    .discipline
                    .audio()
                    .expect("audio timing")
                    .authority();
                if let Some(prepared) = authority.prepare_coordinate_resume_control_cutoff(
                    staged.epoch(),
                    query,
                    at,
                    now,
                    merger,
                )? {
                    let boundary = session
                        .end
                        .as_ref()
                        .ok_or("coordinate end owner disappeared")?
                        .validate_coordinate_end(staged, authority, &prepared, now, merger)?;
                    let basis = session
                        .runtime
                        .transport_mut()
                        .output_phase_basis()
                        .ok_or("coordinate end lacks phase")?;
                    let phase = crate::native_end::coordinate_final_phase(boundary, basis)?;
                    let audio_at = schedule(device, session, *config, render_evidence, stops)?;
                    let report = session.runtime.advance_to_output_phase(
                        prepared.output(),
                        &ExplicitDomains,
                        audio_at,
                        phase,
                    )?;
                    let staged = controls.end.take().expect("preflighted endpoint");
                    session
                        .end
                        .as_mut()
                        .expect("preflighted endpoint owner")
                        .commit_coordinate_end(
                            staged,
                            session
                                .discipline
                                .audio()
                                .expect("audio timing")
                                .authority(),
                            &prepared,
                            now,
                            merger,
                        )?;
                    session
                        .discipline
                        .audio_mut()
                        .expect("audio timing")
                        .authority_mut()
                        .commit_coordinate_control_cutoff(prepared, now, merger)?;
                    controls.end_committed = true;
                    controls.end_host = None;
                    *end_boundary = Some(EndBoundary {
                        host: boundary.host,
                        output: boundary.raw_output,
                        physical_frame: boundary.physical_frame,
                        playback_frame: boundary.playback_frame,
                    });
                    *last_song = report.song_time;
                    *last_host = boundary.host;
                    *terminal = true;
                    publish_with_host(session, report, stops, host)?;
                }
            }
        }
    }
    if *pause_committed && session.pause.phase() == PausePhase::Paused {
        if let Some(frontier) = session
            .discipline
            .audio()
            .expect("audio timing")
            .authority()
            .prepare_coordinate_held_frontier(now, merger)?
        {
            *last_host = frontier.host();
            session
                .discipline
                .audio_mut()
                .expect("audio timing")
                .authority_mut()
                .commit_coordinate_held_frontier(frontier, now, merger)?;
        }
    }
    if controls.end_committed {
        if let Some(prefix) = session
            .discipline
            .audio()
            .expect("audio timing")
            .authority()
            .acquired_prefix()
        {
            while merger.peek_ready(prefix)?.is_some() {
                merger.pop_ready(prefix)?;
            }
        }
        if let Some(frontier) = session
            .discipline
            .audio()
            .expect("audio timing")
            .authority()
            .prepare_coordinate_held_frontier(now, merger)?
        {
            *last_host = frontier.host();
            session
                .discipline
                .audio_mut()
                .expect("audio timing")
                .authority_mut()
                .commit_coordinate_held_frontier(frontier, now, merger)?;
        }
    }
    Ok(())
}

fn service_paused_coordinate_solo<
    D: crate::gameplay_presentation::GameplayDevice,
    S: SoloCompetitionPort,
    H: NativeGameplayHost,
    T: PumpTiming<D>,
>(
    device: &mut D,
    session: &mut GameplaySession<'_, S, T>,
    config: &mut NativeGameplayConfig,
    merger: &mut InputMerger,
    now: ClockPoint,
    keyboard: &mut PauseKeyboard,
    last_host_operation: &mut ClockPoint,
    end_boundary: Option<EndBoundary>,
    host: &H,
    practice_pending: bool,
) -> NativeGameplayResult<()> {
    if let Some(prefix) = session
        .discipline
        .audio()
        .expect("audio timing")
        .authority()
        .acquired_prefix()
    {
        while let Some(event) = merger.pop_ready(prefix)? {
            keyboard.observe_paused(event)?;
        }
    }
    if !device.output_clock_suspended() {
        if let Some(frontier) = session
            .discipline
            .audio()
            .expect("audio timing")
            .authority()
            .prepare_coordinate_held_frontier(now, merger)?
        {
            *last_host_operation = frontier.host();
            session
                .discipline
                .audio_mut()
                .expect("audio timing")
                .authority_mut()
                .commit_coordinate_held_frontier(frontier, now, merger)?;
        }
    }
    if end_boundary.is_none() && !host.cancelled() {
        session.discipline.publish(
            device,
            crate::gameplay_presentation::GameplayPauseControl::solo(session.runtime),
            Some(merger),
            session.pause,
            config,
            session.end,
            now,
            practice_pending,
        )?;
    }
    Ok(())
}

fn service_audio_solo<
    D: crate::gameplay_presentation::GameplayDevice,
    S: SoloCompetitionPort,
    H: NativeGameplayHost,
    T: PumpTiming<D>,
>(
    device: &mut D,
    session: &mut GameplaySession<'_, S, T>,
    config: &mut NativeGameplayConfig,
    merger: &mut InputMerger,
    now: ClockPoint,
    backlog: bool,
    keyboard: &mut PauseKeyboard,
    paused: &mut Option<LivePauseBoundary>,
    transition: &mut Option<AudioLivePauseBoundary>,
    pause_committed: &mut bool,
    last_song: &mut Timestamp,
    last_host_operation: &mut ClockPoint,
    end_rendered: bool,
    end_boundary: Option<EndBoundary>,
    stops: &mut OwnedStopEvidence,
    host: &mut H,
    practice_cut: Option<ClockPoint>,
    practice_source_cut: Option<u64>,
    practice_end: Option<Timestamp>,
    render_evidence: &mut NativeRenderEvidence,
    mut coordinate_controls: Option<&mut SoloCoordinateControls>,
    coordinate_end_boundary: &mut Option<EndBoundary>,
    coordinate_terminal: &mut bool,
) -> NativeGameplayResult<()> {
    let source_history = session.discipline.source_playback_identity();
    if source_history.is_some() && device.output_clock_suspended() {
        return Ok(());
    }
    let Some(prefix) = session
        .discipline
        .audio()
        .expect("audio timing")
        .authority()
        .acquired_prefix()
    else {
        return Ok(());
    };
    if source_history.is_some()
        && matches!(
            session.pause.phase(),
            PausePhase::Pausing | PausePhase::Resuming
        )
        && coordinate_controls
            .as_deref()
            .is_none_or(|controls| controls.pause.is_none())
    {
        return Ok(());
    }

    loop {
        let Some(event) = merger.peek_ready(prefix)? else {
            break;
        };
        let at = point(event);
        if let Some(controls) = coordinate_controls
            .as_deref()
            .filter(|_| source_history.is_some())
        {
            if let Some(adoption) = controls.pause.as_ref() {
                let Some(cut) = controls.pause_host else {
                    break;
                };
                if (!adoption.paused() && at.timestamp >= cut.timestamp)
                    || (adoption.paused() && at.timestamp > cut.timestamp)
                {
                    break;
                }
                if adoption.paused() && at.timestamp == cut.timestamp {
                    keyboard.observe_paused(
                        merger.pop_ready(prefix)?.expect("peeked pause-cut input"),
                    )?;
                    continue;
                }
            }
            if controls
                .end
                .as_ref()
                .is_some_and(|end| end.query().is_some())
            {
                let Some(cut) = controls.end_host else {
                    break;
                };
                if at.timestamp >= cut.timestamp {
                    break;
                }
            }
        }
        if practice_cut.is_some_and(|cut| at.timestamp >= cut.timestamp) {
            break;
        }
        if transition.is_some_and(|boundary| {
            !boundary.original.paused && at.timestamp >= boundary.original.at.timestamp
        }) {
            break;
        }
        if paused.is_some_and(|boundary| at.timestamp >= boundary.at.timestamp) {
            keyboard.observe_paused(merger.pop_ready(prefix)?.expect("peeked paused input"))?;
            continue;
        }
        if end_boundary.is_some_and(|boundary| at.timestamp >= boundary.host.timestamp) {
            merger.pop_ready(prefix)?;
            continue;
        }
        if let Some(identity) = source_history {
            let Some(prepared) = session
                .discipline
                .audio()
                .expect("audio timing")
                .authority()
                .prepare_coordinate_input(at, now)?
            else {
                break;
            };
            if prepared.is_before_origin() {
                let count = session
                    .pre_origin_inputs
                    .checked_add(1)
                    .ok_or("native pre-origin input counter overflow")?;
                session
                    .discipline
                    .audio_mut()
                    .expect("audio timing")
                    .authority_mut()
                    .commit_discarded_coordinate_input(prepared, now)?;
                merger
                    .pop_ready(prefix)?
                    .expect("prepared pre-origin input");
                *session.pre_origin_inputs = count;
                continue;
            }
            let Some(mapping) = device.conditional_playback_mapping_at(
                session.discipline.audio().expect("audio timing"),
                &prepared.query(),
                identity,
            )?
            else {
                break;
            };
            let phase = mapping.phase(
                session.discipline.audio().expect("audio timing"),
                identity,
                &prepared.query(),
                session
                    .runtime
                    .transport_mut()
                    .output_phase_basis()
                    .ok_or("coordinate Runtime lacks output phase")?,
                prepared.output(),
            )?;
            if let Some(cut) = practice_source_cut {
                if !mapping.compare_physical_source_frame(cut)?.is_lt() {
                    break;
                }
            }
            if let Some(end) = practice_end {
                if session
                    .runtime
                    .transport_mut()
                    .position_at_output_phase(phase)?
                    >= end
                {
                    break;
                }
            }
            session
                .discipline
                .audio()
                .expect("audio timing")
                .authority()
                .validate_coordinate_input(&prepared, now)?;
            if config.pause_supported && !keyboard.accept(event)? {
                merger.pop_ready(prefix)?;
                continue;
            }
            let audio_at = schedule(device, session, *config, render_evidence, stops)?;
            if audio_at.domain != config.stream_origin.domain {
                return Err("native audio scheduling domain differs".into());
            }
            let event = merger
                .pop_ready(prefix)?
                .expect("prepared earliest original input");
            let report = session.runtime.process_input_output_phase(
                event,
                prepared.mapper(),
                audio_at,
                phase,
            )?;
            session
                .discipline
                .audio_mut()
                .expect("audio timing")
                .authority_mut()
                .commit_coordinate_input(prepared, now)
                .expect("private current coordinate input token was preflighted");
            *last_song = report.song_time;
            *last_host_operation = at;
            publish_with_host(session, report, stops, host)?;
            continue;
        }
        let Some(prepared) = session
            .discipline
            .audio()
            .expect("audio timing")
            .authority()
            .prepare_input(at, now)?
        else {
            break;
        };
        if prepared.is_before_origin() {
            let count = session
                .pre_origin_inputs
                .checked_add(1)
                .ok_or("native pre-origin input counter overflow")?;
            session
                .discipline
                .audio_mut()
                .expect("audio timing")
                .authority_mut()
                .commit_discarded_input(prepared)?;
            merger
                .pop_ready(prefix)?
                .expect("prepared pre-origin input");
            *session.pre_origin_inputs = count;
            continue;
        }
        // Rendering [start, end) can stop exactly at the pass endpoint
        // before the next block publishes its genuine boundary receipt.
        // Preserve equal/later original inputs until that receipt arrives.
        if let Some(end) = practice_end {
            if session
                .runtime
                .transport_mut()
                .position_at(prepared.output().timestamp)?
                >= end
            {
                break;
            }
        }
        if config.pause_supported && !keyboard.accept(event)? {
            merger.pop_ready(prefix)?;
            continue;
        }
        let audio_at = schedule(device, session, *config, render_evidence, stops)?;
        if audio_at.domain != config.stream_origin.domain {
            return Err("native audio scheduling domain differs".into());
        }
        let event = merger
            .pop_ready(prefix)?
            .expect("prepared earliest original input");
        let report = session
            .runtime
            .process_input(event, prepared.mapper(), audio_at)?;
        session
            .discipline
            .audio_mut()
            .expect("audio timing")
            .authority_mut()
            .commit_input(prepared)
            .expect("private current input token was preflighted");
        *last_song = report.song_time;
        *last_host_operation = at;
        publish_with_host(session, report, stops, host)?;
    }
    if practice_cut.is_some() {
        return Ok(());
    }
    if source_history.is_some() {
        if let Some(controls) = coordinate_controls.as_deref_mut() {
            service_coordinate_solo_control(
                device,
                session,
                config,
                merger,
                now,
                backlog,
                keyboard,
                paused,
                pause_committed,
                last_song,
                last_host_operation,
                controls,
                coordinate_end_boundary,
                coordinate_terminal,
                stops,
                host,
                render_evidence,
            )?;
            if controls.pause.is_some()
                || controls
                    .end
                    .as_ref()
                    .is_some_and(|end| end.query().is_some())
                || controls.end_committed
                || (*pause_committed && session.pause.phase() == PausePhase::Paused)
            {
                return Ok(());
            }
        }
    }
    if !backlog {
        if let Some(boundary) = *transition {
            let prepared = if boundary.original.paused {
                session
                    .discipline
                    .audio()
                    .expect("audio timing")
                    .authority()
                    .prepare_control_cutoff(
                        boundary.epoch,
                        boundary.raw_output,
                        boundary.original.at,
                        now,
                        merger,
                    )?
            } else {
                session
                    .discipline
                    .audio()
                    .expect("audio timing")
                    .authority()
                    .prepare_resume_control_cutoff(
                        boundary.epoch,
                        boundary.raw_output,
                        boundary.original.at,
                        now,
                        merger,
                    )?
            };
            if let Some(prepared) = prepared {
                let transport = if session.discipline.live_speed().is_some() {
                    crate::live_pause::prepare_live_audio_transport_with_speed(
                        session.runtime.transport_mut(),
                        boundary,
                        &prepared,
                        *last_song,
                        config.sample_rate,
                    )?
                } else {
                    prepare_live_audio_transport(
                        session.runtime.transport_mut(),
                        boundary,
                        &prepared,
                        *last_song,
                    )?
                };
                let audio_at = schedule(device, session, *config, render_evidence, stops)?;
                *session.runtime.transport_mut() = transport;
                let report =
                    session
                        .runtime
                        .advance_to(prepared.output(), &ExplicitDomains, audio_at)?;
                session
                    .discipline
                    .audio_mut()
                    .expect("audio timing")
                    .authority_mut()
                    .commit_control_cutoff(prepared, merger)
                    .expect("private current native control token was preflighted");
                *last_song = report.song_time;
                *last_host_operation = boundary.original.at;
                publish_with_host(session, report, stops, host)?;
                if boundary.original.paused {
                    *pause_committed = true;
                    host.publish_pause(PauseState::Paused);
                } else {
                    for event in keyboard.resume(boundary.original.at)? {
                        let prepared = session
                            .discipline
                            .audio()
                            .expect("audio timing")
                            .authority()
                            .prepare_resume_control_cutoff(
                                boundary.epoch,
                                boundary.raw_output,
                                boundary.original.at,
                                now,
                                merger,
                            )?
                            .ok_or("accepted resume lost its authorized control mapping")?;
                        let mapper = AudioControlMapper(prepared);
                        let report = session.runtime.process_input(event, &mapper, audio_at)?;
                        session
                            .discipline
                            .audio_mut()
                            .expect("audio timing")
                            .authority_mut()
                            .commit_control_cutoff(prepared, merger)
                            .expect("private reconciliation control token remains current");
                        *last_song = report.song_time;
                        publish_with_host(session, report, stops, host)?;
                    }
                    *paused = None;
                    *pause_committed = false;
                    host.publish_pause(PauseState::Running);
                }
                *transition = None;
            }
        }
    }
    if *pause_committed && session.pause.phase() == PausePhase::Paused && !end_rendered {
        if !device.output_clock_suspended() {
            if let Some(frontier) = session
                .discipline
                .audio()
                .expect("audio timing")
                .authority()
                .prepare_held_frontier(now, merger)?
            {
                *last_host_operation = frontier.host();
                session
                    .discipline
                    .audio_mut()
                    .expect("audio timing")
                    .authority_mut()
                    .commit_held_frontier(frontier, merger)
                    .expect("private held frontier remains current");
            }
        }
        if end_boundary.is_none() && !host.cancelled() {
            if session.discipline.live_speed().is_some() && device.output_replacement_pending() {
                return Err("selected live speed cannot replace a retained output epoch".into());
            }
            session.discipline.publish(
                device,
                crate::gameplay_presentation::GameplayPauseControl::solo(session.runtime),
                Some(merger),
                session.pause,
                config,
                session.end,
                now,
                false,
            )?;
        }
        return Ok(());
    }
    if transition.is_some()
        || matches!(
            session.pause.phase(),
            PausePhase::Pausing | PausePhase::Resuming
        )
    {
        return Ok(());
    }
    if let Some(identity) = source_history {
        let Some(frontier) = session
            .discipline
            .audio()
            .expect("audio timing")
            .authority()
            .prepare_coordinate_frontier(now, merger)?
        else {
            return Ok(());
        };
        let Some(mapping) = device.conditional_playback_mapping_at(
            session.discipline.audio().expect("audio timing"),
            &frontier.query(),
            identity,
        )?
        else {
            return Ok(());
        };
        let phase = mapping.phase(
            session.discipline.audio().expect("audio timing"),
            identity,
            &frontier.query(),
            session
                .runtime
                .transport_mut()
                .output_phase_basis()
                .ok_or("coordinate Runtime lacks output phase")?,
            frontier.output(),
        )?;
        if let Some(cut) = practice_source_cut {
            if !mapping.compare_physical_source_frame(cut)?.is_lt() {
                return Ok(());
            }
        }
        let report = if let Some(output) = frontier.advance() {
            if let Some(end) = practice_end {
                if session
                    .runtime
                    .transport_mut()
                    .position_at_output_phase(phase)?
                    >= end
                {
                    return Ok(());
                }
            }
            let audio_at = schedule(device, session, *config, render_evidence, stops)?;
            Some(session.runtime.advance_to_output_phase(
                output,
                &ExplicitDomains,
                audio_at,
                phase,
            )?)
        } else {
            None
        };
        *last_host_operation = frontier.host();
        session
            .discipline
            .audio_mut()
            .expect("audio timing")
            .authority_mut()
            .commit_coordinate_frontier(frontier, now, merger)
            .expect("private current coordinate frontier was preflighted");
        if let Some(report) = report {
            *last_song = report.song_time;
            publish_with_host(session, report, stops, host)?;
        }
        return Ok(());
    }
    if let Some(frontier) = session
        .discipline
        .audio()
        .expect("audio timing")
        .authority()
        .prepare_frontier(now, merger)?
    {
        let report = if let Some(output) = frontier.advance() {
            if let Some(end) = practice_end {
                if session
                    .runtime
                    .transport_mut()
                    .position_at(output.timestamp)?
                    >= end
                {
                    return Ok(());
                }
            }
            let audio_at = schedule(device, session, *config, render_evidence, stops)?;
            Some(
                session
                    .runtime
                    .advance_to(output, &ExplicitDomains, audio_at)?,
            )
        } else {
            None
        };
        *last_host_operation = frontier.host();
        session
            .discipline
            .audio_mut()
            .expect("audio timing")
            .authority_mut()
            .commit_frontier(frontier, merger)
            .expect("private current frontier was preflighted");
        if let Some(report) = report {
            *last_song = report.song_time;
            publish_with_host(session, report, stops, host)?;
        }
    }
    Ok(())
}

pub fn run_gameplay_audio_with_result_and_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
>(
    device: &mut D,
    audio: AudioGameplaySession<'_, S>,
    config: AudioGameplayConfig,
    control: &mut C,
    host: &mut H,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    run_gameplay_audio_with_result_and_ports_impl(device, audio, config, control, host, None, None)
}

/// Runs one original-audio solo owner using the exact selected cold speed setup.
pub fn run_gameplay_audio_with_result_and_ports_with_speed<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
>(
    device: &mut D,
    audio: AudioGameplaySession<'_, S>,
    config: AudioGameplayConfig,
    control: &mut C,
    host: &mut H,
    speed: LiveSpeedSetup,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    run_gameplay_audio_with_result_and_ports_impl(
        device,
        audio,
        config,
        control,
        host,
        Some(speed),
        None,
    )
}

/// Runs the same solo pump with an explicit exact-coordinate history binding.
pub fn run_gameplay_audio_coordinate_with_result_and_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
>(
    device: &mut D,
    audio: AudioGameplaySession<'_, S>,
    config: CoordinateGameplayConfig,
    control: &mut C,
    host: &mut H,
    speed: Option<LiveSpeedSetup>,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    run_gameplay_audio_with_result_and_ports_impl(
        device,
        audio,
        config.audio,
        control,
        host,
        speed,
        Some(config.source_history),
    )
}

fn run_gameplay_audio_with_result_and_ports_impl<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
>(
    device: &mut D,
    audio: AudioGameplaySession<'_, S>,
    config: AudioGameplayConfig,
    control: &mut C,
    host: &mut H,
    speed: Option<LiveSpeedSetup>,
    history: Option<crate::gameplay::output::ports::SourcePlaybackIdentity>,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    if let Some(source_history) = history {
        crate::native_policy_admission::validate_audio_coordinate_start(
            &CoordinateGameplayConfig {
                audio: config,
                source_history,
            },
            audio.session.discipline,
            audio.session.runtime.transport_mut(),
            speed,
        )?;
    } else if let Some(speed) = speed {
        crate::native_policy_admission::validate_audio_config_with_speed(&config, speed)?;
    } else {
        crate::native_policy_admission::validate_audio_config(&config)?;
    }
    let section_start = config.section_start;
    let config = config.gameplay;
    let session = audio.session;
    let mut timing = AudioTiming(session.discipline, section_start, speed, history);
    let session = GameplaySession {
        runtime: session.runtime,
        gauge: session.gauge,
        bgm: session.bgm,
        discipline: &mut timing,
        pause: session.pause,
        end: session.end,
        completion: session.completion,
        capture: session.capture,
        competition: session.competition,
        delivery: session.delivery,
        pre_origin_inputs: session.pre_origin_inputs,
    };
    run_gameplay_timed(
        device,
        session,
        Some(audio.merger),
        config,
        control,
        host,
        None::<RetainedSoloPractice<crate::practice_playback::NoopPracticeRecording>>,
    )
}
pub fn run_gameplay_audio_with_policy_and_result_and_score_and_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
>(
    device: &mut D,
    audio: AudioGameplaySession<'_, S>,
    config: AudioGameplayConfig,
    control: &mut C,
    host: &mut H,
    score: &mut crate::competition::ScoreSummary,
    policy: &crate::play_policy::ResolvedPlayPolicy,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    run_gameplay_audio_with_policy_and_result_and_score_and_ports_impl(
        device, audio, config, control, host, score, policy, None, None,
    )
}

/// Runs selected-policy solo gameplay with source-speed identity and original timing.
pub fn run_gameplay_audio_with_policy_and_result_and_score_and_ports_with_speed<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
>(
    device: &mut D,
    audio: AudioGameplaySession<'_, S>,
    config: AudioGameplayConfig,
    control: &mut C,
    host: &mut H,
    score: &mut crate::competition::ScoreSummary,
    policy: &crate::play_policy::ResolvedPlayPolicy,
    speed: LiveSpeedSetup,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    run_gameplay_audio_with_policy_and_result_and_score_and_ports_impl(
        device,
        audio,
        config,
        control,
        host,
        score,
        policy,
        Some(speed),
        None,
    )
}

/// Coordinate-bound selected policy and scoring through the existing solo owner.
pub fn run_gameplay_audio_coordinate_with_policy_and_result_and_score_and_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
>(
    device: &mut D,
    audio: AudioGameplaySession<'_, S>,
    config: CoordinateGameplayConfig,
    control: &mut C,
    host: &mut H,
    score: &mut crate::competition::ScoreSummary,
    policy: &crate::play_policy::ResolvedPlayPolicy,
    speed: Option<LiveSpeedSetup>,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    run_gameplay_audio_with_policy_and_result_and_score_and_ports_impl(
        device,
        audio,
        config.audio,
        control,
        host,
        score,
        policy,
        speed,
        Some(config.source_history),
    )
}

fn run_gameplay_audio_with_policy_and_result_and_score_and_ports_impl<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
>(
    device: &mut D,
    mut audio: AudioGameplaySession<'_, S>,
    config: AudioGameplayConfig,
    control: &mut C,
    host: &mut H,
    score: &mut crate::competition::ScoreSummary,
    policy: &crate::play_policy::ResolvedPlayPolicy,
    speed: Option<LiveSpeedSetup>,
    history: Option<crate::gameplay::output::ports::SourcePlaybackIdentity>,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    if let Some(source_history) = history {
        crate::native_policy_admission::validate_audio_coordinate_start(
            &CoordinateGameplayConfig {
                audio: config,
                source_history,
            },
            audio.session.discipline,
            audio.session.runtime.transport_mut(),
            speed,
        )?;
    } else if let Some(speed) = speed {
        crate::native_policy_admission::validate_audio_config_with_speed(&config, speed)?;
    } else {
        crate::native_policy_admission::validate_audio_config(&config)?;
    }
    let section_start = config.section_start;
    let config = config.gameplay;
    if score != &crate::competition::ScoreSummary::default() {
        return Err("native scored audio gameplay requires a fresh score".into());
    }
    let session = &mut audio.session;
    let epoch = session.discipline.authority().epoch();
    if session.runtime.clock_domains()
        != Some((epoch.logical_origin.domain, epoch.stream_origin.domain))
        || epoch.host_domain != config.origin.domain
        || epoch.stream_origin != config.stream_origin
        || session.runtime.transport_mut().anchor().host_time
            != epoch.logical_output(config.playback_origin)?.timestamp
        || session.runtime.transport_mut().anchor().song_time != config.song_origin
        || session.runtime.transport_mut().anchor().rate
            != speed.map_or(beatkernel::transport::Rate::NORMAL, |setup| {
                setup.speed().rate()
            })
        || audio.merger.source_capacity() != 4096
    {
        return Err("selected audio Runtime domains, anchor or acquisition bound differ".into());
    }
    if let Some(speed) = speed {
        if session.runtime.song_end() != speed.end()
            || session.end.is_some() != speed.end().is_some()
            || session.pause.epoch() != epoch.id
            || session.pause.host_domain() != epoch.host_domain
            || session.pause.phase() != PausePhase::Running
        {
            return Err(
                "selected live speed requires matching original end and pristine pause epoch"
                    .into(),
            );
        }
        if history.is_none() {
            crate::native_policy_admission::validate_transport_with_speed(
                session.runtime.transport_mut(),
                epoch.logical_output(config.playback_origin)?.timestamp,
                speed,
            )?;
        }
    }
    let rows = [(crate::local_players::PlayerId(1), policy)];
    crate::native_gameplay_host::validate_play_policy_members(&rows)?;
    crate::native_policy_admission::validate_config(&config)?;
    let header = session
        .competition
        .as_ref()
        .filter(|port| !port.policy_agnostic())
        .map(|port| {
            port.expected_policy_header()
                .ok_or("selected audio competition has no policy identity")
        })
        .transpose()?;
    if let Some(speed) = speed {
        crate::native_policy_admission::validate_selected_in_domain_with_speed(
            session.runtime.judge(),
            session.gauge,
            policy,
            session.capture.as_ref(),
            header,
            &config,
            epoch.logical_origin.domain,
            Some(section_start),
            speed,
        )?;
    } else {
        crate::native_policy_admission::validate_selected_in_domain(
            session.runtime.judge(),
            session.gauge,
            policy,
            session.capture.as_ref(),
            header,
            &config,
            epoch.logical_origin.domain,
            Some(section_start),
        )?;
    }
    if session.runtime.poisoned() || session.runtime.gameplay_fence().is_some() {
        return Err("selected audio Runtime is poisoned or fenced".into());
    }
    if speed.is_none() && policy.gauge() == &GaugeProfile::default() {
        host.prepare_play_policies(&rows)?;
    }
    let mut resolved = crate::native_gameplay_bridge::ResolvedGameplayHost {
        host,
        policies: &rows,
    };
    let mut observer = crate::native_gameplay_host::NativeScoreHost::new(&mut resolved, score);
    run_gameplay_audio_with_result_and_ports_impl(
        device,
        audio,
        AudioGameplayConfig {
            gameplay: config,
            section_start,
        },
        control,
        &mut observer,
        speed,
        history,
    )
}

#[cfg(test)]
#[path = "native_audio_authority_fixtures.rs"]
mod native_audio_authority_fixtures;

#[cfg(test)]
#[path = "native_solo_coordinate_pump_fixtures.rs"]
mod native_solo_coordinate_pump_fixtures;

#[cfg(test)]
#[path = "native_solo_coordinate_control_fixtures.rs"]
mod native_solo_coordinate_control_fixtures;

#[cfg(test)]
#[path = "native_solo_target_practice_fixtures.rs"]
mod native_solo_target_practice_fixtures;

#[cfg(test)]
#[path = "native_solo_source_practice_fixtures.rs"]
mod native_solo_source_practice_fixtures;
