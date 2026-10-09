//! Shared local-cohort gameplay policy over original native input and evidence.
#[cfg(test)]
use crate::native_gameplay::NativeGameplayDevice;
pub use crate::native_gameplay_bridge::{
    finite_cohort_done, member_progress, run_cohort, run_cohort_audio_with_policies_and_results,
    run_cohort_audio_with_practice_and_policies_and_results, run_cohort_audio_with_results,
    run_cohort_with_control, run_cohort_with_policies_and_results, run_cohort_with_results,
    NativeAudioCohortSession, NativeCohortSession, PlayerState,
};
#[cfg(test)]
use crate::native_group_competition::NativeGroupCompetition;
use crate::{
    bgm::{BgmFeedReport, BgmFeeder},
    competition::ScoreSummary,
    completion::SongCompletion,
    gameplay_competition::{GroupCompetitionPort, SoloCompetitionPort},
    gameplay_presentation::{AudioTiming, LegacyTiming, PumpTiming},
    gauge::{BmsGauge, GaugeProfile},
    live_pause::{
        prepare_live_audio_transport, prepare_live_transport, update_live_audio_pause,
        update_live_pause, validate_pre_pause_input, AudioLivePauseBoundary, LivePauseBoundary,
    },
    local_input::InputMerger,
    local_players::PlayerId,
    local_runtime::{GroupError, InputResult, PlayerReport, RuntimeGroup},
    multiplayer::Progress,
    multiplayer_group::{validate_members, MemberProgress},
    native_audio::{finite_terminal_output_ready, validate_stop_evidence, NativeStopBarrier},
    native_end::NativeEnd,
    native_gameplay::{
        AudioGameplayConfig, NativeGameplayConfig, NativeGameplayResult, MAX_PENDING_INPUT_EVENTS,
    },
    native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost, PauseState},
    native_pump_control::{NativePumpControl, NativePumpDeadline},
    offline::OwnedStopEvidence,
    play_result::{CompletedLocalPublicationError, CompletedPlayResult},
    playback_pause::{NativePause, PauseKeyboard, PausePhase},
    replay_capture::LiveReplayCapture,
};
#[cfg(test)]
use beatkernel::time::presentation::DisciplineConfig;
use beatkernel::{
    audio::RenderReport,
    input::PhysicalInputEvent,
    telemetry::InputDeliveryTelemetry,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
};
#[cfg(test)]
use beatkernel_platform::audio::presentation::discipline::PresentationDiscipline;
use std::{
    collections::VecDeque,
    fmt,
    path::{Path, PathBuf},
    time::Duration as WallDuration,
};

/// Borrowed cohort state with explicit per-member and shared competition ports.
pub struct CohortSession<'a, S, G, P> {
    pub group: &'a mut RuntimeGroup,
    pub network: Option<&'a mut G>,
    pub states: &'a mut [GameplayPlayerState<S>],
    pub merger: &'a mut InputMerger,
    pub bgm: &'a mut BgmFeeder,
    pub discipline: &'a mut P,
    pub pause: &'a mut NativePause,
    pub end: &'a mut Option<NativeEnd>,
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
fn schedule<D: crate::gameplay_presentation::GameplayDevice, S, G, P>(
    device: &mut D,
    session: &CohortSession<'_, S, G, P>,
    config: NativeGameplayConfig,
) -> NativeGameplayResult<ClockPoint> {
    if config.logical_schedule {
        Ok(session.pause.scheduling_point(
            device
                .render_report()?
                .or_else(|| session.pause.last_render_report())
                .ok_or("cohort playback boundary unavailable")?,
        )?)
    } else {
        device.fallback_schedule(config.sample_rate)
    }
}
#[cfg(test)]
fn process<D: NativeGameplayDevice>(
    device: &mut D,
    session: &mut NativeCohortSession<'_>,
    config: NativeGameplayConfig,
    event: PhysicalInputEvent,
) -> NativeGameplayResult<()> {
    process_with_stops(
        device,
        session,
        config,
        event,
        &mut OwnedStopEvidence::default(),
    )
}
#[cfg(test)]
fn process_with_stops<D: NativeGameplayDevice>(
    device: &mut D,
    session: &mut NativeCohortSession<'_>,
    config: NativeGameplayConfig,
    event: PhysicalInputEvent,
    evidence: &mut OwnedStopEvidence,
) -> NativeGameplayResult<()> {
    process_with_host(
        device,
        session,
        config,
        event,
        evidence,
        &mut crate::native_gameplay_bridge::PlayerGameplayHost,
    )
}
fn process_with_host<
    D: crate::gameplay_presentation::GameplayDevice,
    S: SoloCompetitionPort,
    G: GroupCompetitionPort,
    H: NativeGameplayHost,
    P,
>(
    device: &mut D,
    session: &mut CohortSession<'_, S, G, P>,
    config: NativeGameplayConfig,
    event: PhysicalInputEvent,
    evidence: &mut OwnedStopEvidence,
    host_port: &mut H,
) -> NativeGameplayResult<()> {
    let at = schedule(device, session, config)?;
    match session.group.process_input(event, &ExplicitDomains, at) {
        Ok(InputResult::Processed(mut reports)) => observe_reports_with_competition(
            &mut reports,
            session.states,
            session.group,
            session.network.as_deref_mut(),
            evidence,
            host_port,
        ),
        Ok(InputResult::Ignored { device }) => {
            Err(format!("merged source {device:?} has no cohort owner").into())
        }
        Err(mut error) => {
            let observation_error = observe_reports_with_competition(
                &mut error.completed_reports,
                session.states,
                session.group,
                session.network.as_deref_mut(),
                evidence,
                host_port,
            )
            .err();
            Err(Box::new(NativeCohortProcessingError {
                group_error: error,
                observation_error,
            }))
        }
    }
}
fn advance<
    D: crate::gameplay_presentation::GameplayDevice,
    S: SoloCompetitionPort,
    G: GroupCompetitionPort,
    H: NativeGameplayHost,
    P,
>(
    device: &mut D,
    session: &mut CohortSession<'_, S, G, P>,
    config: NativeGameplayConfig,
    at: ClockPoint,
    evidence: &mut OwnedStopEvidence,
    host_port: &mut H,
) -> NativeGameplayResult<()> {
    let audio_at = schedule(device, session, config)?;
    match session.group.advance_to(at, &ExplicitDomains, audio_at) {
        Ok(mut reports) => observe_reports_with_competition(
            &mut reports,
            session.states,
            session.group,
            session.network.as_deref_mut(),
            evidence,
            host_port,
        ),
        Err(mut error) => {
            let observation_error = observe_reports_with_competition(
                &mut error.completed_reports,
                session.states,
                session.group,
                session.network.as_deref_mut(),
                evidence,
                host_port,
            )
            .err();
            Err(Box::new(NativeCohortProcessingError {
                group_error: error,
                observation_error,
            }))
        }
    }
}
fn reconcile<
    D: crate::gameplay_presentation::GameplayDevice,
    S: SoloCompetitionPort,
    G: GroupCompetitionPort,
    H: NativeGameplayHost,
    P,
>(
    device: &mut D,
    session: &mut CohortSession<'_, S, G, P>,
    config: NativeGameplayConfig,
    keyboard: &mut PauseKeyboard,
    at: ClockPoint,
    evidence: &mut OwnedStopEvidence,
    host_port: &mut H,
) -> NativeGameplayResult<()> {
    for event in keyboard.resume(at)? {
        process_with_host(device, session, config, event, evidence, host_port)?;
    }
    Ok(())
}
/// One player's retained actual prefix and optional competition observer.
pub struct GameplayPlayerState<S> {
    pub player: PlayerId,
    pub capture: Option<LiveReplayCapture>,
    pub competition: Option<S>,
    pub completion: Option<SongCompletion>,
    pub score: ScoreSummary,
    pub gauge: BmsGauge,
    pub last_song: Timestamp,
}

/// The original committed prefix and all independent observation diagnostics.
#[derive(Debug)]
pub struct NativeCohortObservationError {
    pub reports: Vec<PlayerReport>,
    pub failures: Vec<String>,
}
impl fmt::Display for NativeCohortObservationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("native cohort observation: ")?;
        for (index, failure) in self.failures.iter().enumerate() {
            if index > 0 {
                f.write_str("; ")?;
            }
            f.write_str(failure)?;
        }
        Ok(())
    }
}
impl std::error::Error for NativeCohortObservationError {}

/// Preserves a group failure and any subsequent observation failure together.
#[derive(Debug)]
pub struct NativeCohortProcessingError {
    pub group_error: GroupError,
    pub observation_error: Option<Box<dyn std::error::Error>>,
}
impl fmt::Display for NativeCohortProcessingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.group_error)?;
        if let Some(error) = &self.observation_error {
            write!(f, "; {error}")?;
        }
        Ok(())
    }
}
impl std::error::Error for NativeCohortProcessingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.group_error)
    }
}

/// Snapshot every member's retained committed prefix without advancing gameplay.
pub fn member_progress_for_states<S>(
    states: &[GameplayPlayerState<S>],
) -> NativeGameplayResult<Vec<MemberProgress>> {
    let members = states
        .iter()
        .map(|state| MemberProgress {
            player: state.player,
            progress: Progress {
                song_ns: state.last_song.as_nanos(),
                hits: state.score.hits,
                misses: state.score.misses,
                combo: state.score.combo,
                max_combo: state.score.max_combo,
            },
        })
        .collect::<Vec<_>>();
    validate_members(None, &members)?;
    Ok(members)
}

/// A candidate watermark is insufficient: every member requires the same
/// actually committed acquisition frontier and acknowledged native endpoint.
pub fn finite_cohort_done_for_states<S>(
    end: Option<i64>,
    presented: Option<ClockPoint>,
    committed: Option<ClockPoint>,
    states: &[GameplayPlayerState<S>],
    backlog: bool,
    resuming: bool,
) -> bool {
    if states.is_empty() {
        return false;
    }
    committed.is_some_and(|frontier| {
        states.iter().all(|state| {
            end.is_some_and(|end| state.last_song.as_nanos() >= end)
                && presented.is_some_and(|at| {
                    at.domain == frontier.domain && frontier.timestamp >= at.timestamp
                })
                && !backlog
                && !resuming
        })
    })
}

fn finite_cohort_done_with_terminal<S>(
    end: Option<i64>,
    presented: Option<ClockPoint>,
    committed: Option<ClockPoint>,
    states: &[GameplayPlayerState<S>],
    group: &RuntimeGroup,
    backlog: bool,
    resuming: bool,
    bgm: BgmFeedReport,
    rendered: Option<RenderReport>,
) -> bool {
    let failed = |state: &GameplayPlayerState<S>| {
        state.gauge.snapshot().failure.is_some()
            && group.player_gameplay_fence(state.player).is_some()
    };
    if !states.iter().any(failed) {
        return finite_cohort_done_for_states(end, presented, committed, states, backlog, resuming);
    }
    end.is_some_and(|end| {
        states
            .iter()
            .all(|state| failed(state) || state.last_song.as_nanos() >= end)
    }) && committed.is_some_and(|frontier| {
        presented
            .is_some_and(|at| at.domain == frontier.domain && frontier.timestamp >= at.timestamp)
    }) && !backlog
        && !resuming
        && finite_terminal_output_ready(bgm, rendered, group.admitted_audio_commands())
}

pub fn replay_path(base: &Path, player: PlayerId) -> NativeGameplayResult<PathBuf> {
    if player.0 == 0 {
        return Err("invalid local replay player identity".into());
    }
    let mut stem = base
        .file_stem()
        .filter(|stem| !stem.is_empty())
        .ok_or("local replay base requires a filename")?
        .to_os_string();
    stem.push(format!(".p{}.bkr", player.0));
    Ok(base.with_file_name(stem))
}

#[cfg(test)]
fn observe_reports(
    reports: &mut [PlayerReport],
    states: &mut [PlayerState],
    group: &mut RuntimeGroup,
    network: Option<&mut NativeGroupCompetition>,
) -> NativeGameplayResult<()> {
    observe_reports_with_stops(
        reports,
        states,
        group,
        network,
        &mut OwnedStopEvidence::default(),
    )
}
#[cfg(test)]
fn observe_reports_with_stops(
    reports: &mut [PlayerReport],
    states: &mut [PlayerState],
    group: &mut RuntimeGroup,
    network: Option<&mut NativeGroupCompetition>,
    evidence: &mut OwnedStopEvidence,
) -> NativeGameplayResult<()> {
    observe_reports_with_host(
        reports,
        states,
        group,
        network,
        evidence,
        &mut crate::native_gameplay_bridge::PlayerGameplayHost,
    )
}
#[cfg(test)]
fn observe_reports_with_host<H: NativeGameplayHost>(
    reports: &mut [PlayerReport],
    states: &mut [PlayerState],
    group: &mut RuntimeGroup,
    network: Option<&mut NativeGroupCompetition>,
    evidence: &mut OwnedStopEvidence,
    host_port: &mut H,
) -> NativeGameplayResult<()> {
    observe_reports_with_competition(reports, states, group, network, evidence, host_port)
}
fn observe_reports_with_competition<
    S: SoloCompetitionPort,
    G: GroupCompetitionPort,
    H: NativeGameplayHost,
>(
    reports: &mut [PlayerReport],
    states: &mut [GameplayPlayerState<S>],
    group: &mut RuntimeGroup,
    network: Option<&mut G>,
    evidence: &mut OwnedStopEvidence,
    host_port: &mut H,
) -> NativeGameplayResult<()> {
    let mut failures = Vec::new();
    let mut fences = [None; 64];
    for (index, tagged) in reports.iter().enumerate() {
        let Some(state) = states
            .iter_mut()
            .find(|state| state.player == tagged.player)
        else {
            failures.push(format!("report for unknown player {:?}", tagged.player));
            continue;
        };
        let was_fenced = group.player_gameplay_fence(tagged.player).is_some();
        state.last_song = tagged.report.song_time;
        if let Err(error) = state
            .gauge
            .observe(&tagged.report.judge_events, &tagged.report.hazard_events)
        {
            failures.push(format!("player{} gauge: {error}", state.player.0));
        }
        if !was_fenced {
            if let Some(capture) = state.capture.as_mut() {
                if let Err(error) = capture.record_report(&tagged.report) {
                    failures.push(format!("player{} capture: {error}", state.player.0));
                }
            }
            if state.gauge.snapshot().failure.is_some() {
                fences[index] = Some(state.player);
            }
        }
        if !tagged.report.judge_events.is_empty() {
            if let Err(error) = state.score.observe(&tagged.report.judge_events) {
                failures.push(format!("player{} score: {error}", state.player.0));
            }
        }
        if let Some(competition) = state.competition.as_mut() {
            if let Err(error) = competition.observe(&tagged.report) {
                failures.push(format!("player{} competition: {error}", state.player.0));
            }
        }
        if tagged.report.judge_error.is_some() || !tagged.report.audio_failures.is_empty() {
            failures.push(format!(
                "player{} committed partial report judge={:?}, audio={:?}",
                tagged.player.0, tagged.report.judge_error, tagged.report.audio_failures
            ));
        }
    }
    if let Some(network) = network {
        match member_progress_for_states(states).and_then(|members| network.observe(&members)) {
            Ok(()) => {}
            Err(error) => failures.push(format!("shared competition: {error}")),
        }
    }
    if let Err(error) = host_port.publish_local_reports(reports) {
        failures.push(format!("local presentation: {error}"));
    }
    for (index, player) in fences.into_iter().enumerate() {
        let Some(player) = player else {
            continue;
        };
        if let Err(error) = group.fence_player(player) {
            failures.push(format!("player{} gameplay fence: {error}", player.0));
            continue;
        }
        let report = &mut reports[index].report;
        match group.fence_player_sounds(player, report.audio_at.timestamp) {
            Ok(Some(stops)) => {
                if let Err(error) = evidence.record_admitted(&stops.commands) {
                    failures.push(format!("player{} Stop evidence: {error}", player.0));
                }
                if !stops.commands.is_empty() {
                    // Every member shares this output queue and idle evidence.
                    for state in states.iter_mut() {
                        if let Some(completion) = state.completion.as_mut() {
                            completion.reset_drain();
                        }
                    }
                }
                if !stops.failures.is_empty() {
                    failures.push(format!(
                        "player{} gameplay sound stops: {:?}",
                        player.0, stops.failures
                    ));
                }
                report.audio_commands.extend(stops.commands);
                report.audio_failures.extend(stops.failures);
            }
            Ok(None) => {}
            Err(error) => {
                failures.push(format!("player{} gameplay sound stops: {error}", player.0));
            }
        }
    }
    for tagged in reports.iter() {
        host_port.diagnostic(NativeGameplayDiagnostic::LocalReport {
            player: tagged.player,
            report: &tagged.report,
        });
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(Box::new(NativeCohortObservationError {
            reports: reports.to_vec(),
            failures,
        }))
    }
}

pub fn lag_reaches(now: ClockPoint, boundary: ClockPoint, lag: i64) -> NativeGameplayResult<bool> {
    if now.domain != boundary.domain || !(0..=1_000_000_000).contains(&lag) {
        return Err("local pause lag has invalid domain or extent".into());
    }
    let frontier = i128::from(now.timestamp.as_nanos())
        .checked_sub(i128::from(lag))
        .ok_or("local pause lag arithmetic overflow")?;
    Ok(frontier >= i128::from(boundary.timestamp.as_nanos()))
}

pub fn run_cohort_with_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
    G: GroupCompetitionPort,
>(
    device: &mut D,
    session: CohortSession<'_, S, G, D::Presentation>,
    config: NativeGameplayConfig,
    control: &mut C,
    host_port: &mut H,
) -> NativeGameplayResult<()> {
    run_cohort_with_results_and_ports(device, session, config, control, host_port).map(|_| ())
}

fn complete_cohort<S: SoloCompetitionPort, G: GroupCompetitionPort, P, H: NativeGameplayHost>(
    session: &mut CohortSession<'_, S, G, P>,
    config: NativeGameplayConfig,
    host: &mut H,
) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
    if host.cancelled() {
        return Ok(None);
    }
    let mut results = Vec::new();
    results.try_reserve_exact(session.states.len())?;
    for state in session.states.iter() {
        results.push((
            state.player,
            CompletedPlayResult::from_completed(config.song_origin, config.end_song, &state.gauge),
        ));
    }
    if host.cancelled() {
        return Ok(None);
    }
    for state in session.states.iter_mut() {
        if let Some(competition) = state.competition.as_mut() {
            competition.mark_native_completed();
        }
    }
    if let Some(network) = session.network.as_deref_mut() {
        network.mark_native_completed();
    }
    if let Some(end) = config.end_song {
        host.publish_section_end(end);
    }
    if let Err(cause) = host.publish_completed_local(&results) {
        return Err(Box::new(CompletedLocalPublicationError { results, cause }));
    }
    Ok(Some(results))
}

/// Runs the actual cohort policy with explicit device, clock/wait and host effects.
/// Acquired input and native presentation keep their original clock domains.
/// Full selected original-ID policy admission, independent of capture availability.
pub fn run_cohort_with_policies_and_results_and_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
    G: GroupCompetitionPort,
>(
    device: &mut D,
    session: CohortSession<'_, S, G, D::Presentation>,
    config: NativeGameplayConfig,
    control: &mut C,
    host_port: &mut H,
    policies: &[(PlayerId, &crate::play_policy::ResolvedPlayPolicy)],
) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
    crate::native_gameplay_host::validate_play_policy_members(policies)?;
    crate::native_policy_admission::validate_config(&config)?;
    if !(2..=64).contains(&session.states.len())
        || session.group.poisoned()
        || !session
            .group
            .player_ids()
            .eq(session.states.iter().map(|state| state.player))
        || !session
            .states
            .iter()
            .map(|state| state.player)
            .eq(policies.iter().map(|(id, _)| *id))
    {
        return Err("selected cohort requires the exact pristine original-ID roster".into());
    }
    let custom = policies
        .iter()
        .any(|(_, policy)| policy.gauge() != &GaugeProfile::default());
    if custom
        && session
            .network
            .as_ref()
            .is_some_and(|network| !network.policy_agnostic())
    {
        return Err("nondefault cohort networking requires policy-aware member identities".into());
    }
    for (state, (_, policy)) in session.states.iter().zip(policies) {
        if state.score != ScoreSummary::default()
            || state.last_song != config.song_origin
            || session.group.player_gameplay_fence(state.player).is_some()
        {
            return Err("selected cohort requires pristine member state".into());
        }
        let judge = session
            .group
            .member_judge(state.player)
            .ok_or("selected cohort missing judge")?;
        let competition = state
            .competition
            .as_ref()
            .filter(|port| !port.policy_agnostic());
        let header = competition
            .map(|port| {
                port.expected_policy_header()
                    .ok_or("selected member competition has no policy identity")
            })
            .transpose()?;
        crate::native_policy_admission::validate_selected(
            judge,
            &state.gauge,
            policy,
            state.capture.as_ref(),
            header,
            &config,
        )?;
    }
    if !custom {
        host_port.prepare_play_policies(policies)?;
    }
    let mut host = crate::native_gameplay_bridge::ResolvedGameplayHost {
        host: host_port,
        policies,
    };
    run_cohort_with_results_and_ports(device, session, config, control, &mut host)
}

pub fn run_cohort_with_results_and_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
    G: GroupCompetitionPort,
>(
    device: &mut D,
    session: CohortSession<'_, S, G, D::Presentation>,
    config: NativeGameplayConfig,
    control: &mut C,
    host_port: &mut H,
) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
    let mut timing = LegacyTiming(session.discipline);
    run_cohort_timed(
        device,
        CohortSession {
            group: session.group,
            network: session.network,
            states: session.states,
            merger: session.merger,
            bgm: session.bgm,
            discipline: &mut timing,
            pause: session.pause,
            end: session.end,
            delivery: session.delivery,
            pre_origin_inputs: session.pre_origin_inputs,
        },
        config,
        control,
        host_port,
        None::<(
            &mut crate::practice_playback::PracticePlayback,
            &mut crate::practice_playback::NoopPracticeRecording,
        )>,
    )
}

/// Runs retained original-PCM practice over the same device, input merger and
/// admitted audio authority. The recording port receives each genuine old take.
pub fn run_cohort_audio_practice_with_results_and_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
    G: GroupCompetitionPort,
    R: crate::practice_playback::PracticeRecordingPort,
>(
    device: &mut D,
    session: AudioCohortSession<'_, S, G>,
    config: AudioGameplayConfig,
    control: &mut C,
    host: &mut H,
    practice: &mut crate::practice_playback::PracticePlayback,
    recording: &mut R,
) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
    crate::native_policy_admission::validate_audio_config(&config)?;
    if session.group.song_end() != config.gameplay.end_song
        || config.gameplay.end_song != practice.section_end()
    {
        return Err("retained cohort section endpoints differ".into());
    }
    let mut timing = AudioTiming(session.discipline, config.section_start);
    let states = session.states;
    let result = run_cohort_timed(
        device,
        CohortSession {
            group: session.group,
            network: session.network,
            states: &mut *states,
            merger: session.merger,
            bgm: session.bgm,
            discipline: &mut timing,
            pause: session.pause,
            end: session.end,
            delivery: session.delivery,
            pre_origin_inputs: session.pre_origin_inputs,
        },
        config.gameplay,
        control,
        host,
        Some((&mut *practice, &mut *recording)),
    );
    // The outer native finisher owns the last capture, completed result
    // sidecars and replay-save publication. Only retired attempts archive here.
    let revoke = host.advertise_practice(None);
    match result {
        Err(error) => Err(error),
        Ok(value) => {
            revoke?;
            Ok(value)
        }
    }
}

fn commit_cohort_practice<
    D: crate::gameplay_presentation::GameplayDevice,
    S: SoloCompetitionPort,
    G: GroupCompetitionPort,
    H: NativeGameplayHost,
    T: PumpTiming<D>,
    R: crate::practice_playback::PracticeRecordingPort,
>(
    device: &mut D,
    session: &mut CohortSession<'_, S, G, T>,
    config: &mut NativeGameplayConfig,
    owner: &mut crate::practice_playback::PracticePlayback,
    recording: &mut R,
    boundary: crate::native_audio_presentation::PreparedPracticeBoundary,
    now: ClockPoint,
    stop_evidence: &mut OwnedStopEvidence,
    host: &mut H,
) -> NativeGameplayResult<()> {
    use beatkernel::{
        audio::{CommandScope, PracticeBoundaryKind},
        transport::{Rate, Transport},
    };
    let mut prepared = owner.prepare_boundary(&boundary, host)?;
    if matches!(
        prepared.receipt.kind,
        PracticeBoundaryKind::Requested | PracticeBoundaryKind::Looped
    ) {
        // Prepare every potentially fallible competition owner before retiring
        // captures or changing any live member. Enabled ghosts cannot disappear.
        let mut competitions = Vec::new();
        competitions.try_reserve_exact(session.states.len())?;
        for (state, (player, attempt)) in session.states.iter().zip(&prepared.attempts) {
            if state.player != *player {
                return Err("practice attempt roster differs".into());
            }
            competitions.push(
                state
                    .competition
                    .as_ref()
                    .map(|competition| {
                        competition.prepare_practice(
                            attempt,
                            &owner
                                .members()
                                .iter()
                                .find(|member| member.player == *player)
                                .ok_or("practice policy owner missing")?
                                .policy,
                        )
                    })
                    .transpose()?,
            );
        }
        let song_end = prepared
            .attempts
            .first()
            .ok_or("practice attempt roster empty")?
            .1
            .config
            .end;
        // Retain a cold source-PCM completion template even during finite
        // loops; returning later to full-song scrub must not lose that owner.
        let mut completions = Vec::new();
        completions.try_reserve_exact(session.states.len())?;
        for (state, (_, attempt)) in session.states.iter().zip(&prepared.attempts) {
            completions.push(
                state
                    .completion
                    .as_ref()
                    .map(|template| template.prepare_practice(&attempt.source, &attempt.judge))
                    .transpose()?,
            );
        }
        let mut judges = Vec::new();
        let mut replacements = Vec::new();
        judges.try_reserve_exact(session.states.len())?;
        replacements.try_reserve_exact(session.states.len())?;
        for (((player, attempt), competition), completion) in prepared
            .attempts
            .drain(..)
            .zip(competitions)
            .zip(completions)
        {
            judges.push((player, attempt.judge));
            replacements.push(GameplayPlayerState {
                player,
                capture: attempt.capture,
                competition,
                completion,
                score: attempt.score,
                gauge: attempt.gauge,
                last_song: attempt.config.start,
            });
        }
        // Commit the old attempt's final logical prefix at this shared cut.
        // New input at exactly the cut remains queued for the fresh attempt.
        let audio_at = schedule(device, session, *config)?;
        let result = session
            .group
            .advance_to(boundary.output(), &ExplicitDomains, audio_at);
        observe_audio_group_result(result, session, |_, _| {}, stop_evidence, host)?;
        // Original recording paths are pinned to each old launch, independently
        // of the newly prepared retry identity. Failure remains visible.
        for (state, member) in session.states.iter_mut().zip(owner.members()) {
            if let Some(capture) = state.capture.take() {
                let path = member
                    .launch
                    .args()
                    .windows(2)
                    .find(|pair| pair[0] == "--record-replay")
                    .map(|pair| PathBuf::from(&pair[1]))
                    .ok_or("practice capture has no pinned recording path")?;
                recording.archive(state.player, &path, capture)?;
            }
        }
        let transport = Transport::new(
            boundary.output().timestamp,
            prepared.receipt.applied_song_time,
            Rate::NORMAL,
        );
        session.group.replace_practice_states(
            judges,
            transport,
            song_end,
            CommandScope(prepared.receipt.generation),
        )?;
        for (state, replacement) in session.states.iter_mut().zip(replacements) {
            *state = replacement;
        }
        config.song_origin = prepared.receipt.applied_song_time;
        session
            .discipline
            .set_section_start(prepared.receipt.applied_song_time.max(Timestamp::ZERO));
        // Region on requested controls is available from the prepared attempt,
        // not the still-old owner. It is committed below before further input.
        *session.end = None;
    } else if prepared.receipt.kind == PracticeBoundaryKind::Ended {
        // Advance all members exactly to the native-qualified terminal output.
        let audio_at = schedule(device, session, *config)?;
        let result = session
            .group
            .advance_to(boundary.output(), &ExplicitDomains, audio_at);
        observe_audio_group_result(result, session, |_, _| {}, stop_evidence, host)?;
    }
    session
        .discipline
        .audio_mut()
        .expect("audio timing")
        .commit_practice_boundary(boundary, now, session.merger)?;
    let applied = owner.apply_boundary(&mut prepared)?;
    config.end_song = owner.section_end();
    if let Some(mut presentation) = prepared.presentation.take() {
        host.apply_practice_identity(&mut presentation, applied.receipt().generation);
        host.commit_practice_presentation(presentation, applied.receipt().generation)?;
    }
    owner.publish_boundary(&applied, host)
}

fn run_cohort_timed<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
    G: GroupCompetitionPort,
    T: PumpTiming<D>,
    R: crate::practice_playback::PracticeRecordingPort,
>(
    device: &mut D,
    mut session: CohortSession<'_, S, G, T>,
    mut config: NativeGameplayConfig,
    control: &mut C,
    host_port: &mut H,
    mut practice: Option<(&mut crate::practice_playback::PracticePlayback, &mut R)>,
) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
    if practice.is_some() && (!T::AUDIO || session.network.is_some()) {
        return Err("retained cohort practice requires local audio-authoritative playback".into());
    }
    if let Some((owner, _)) = practice.as_mut() {
        if session.group.song_end() != config.end_song || config.end_song != owner.section_end() {
            return Err("retained cohort section endpoints differ".into());
        }
        if !owner
            .members()
            .iter()
            .map(|member| member.player)
            .eq(session.states.iter().map(|state| state.player))
        {
            return Err("retained practice roster differs from live cohort".into());
        }
    }
    let logical_domain = session.discipline.logical_domain(config);
    let section_start = session.discipline.section_start();
    if let Some(epoch) = session
        .discipline
        .audio()
        .map(|presentation| presentation.authority().epoch())
    {
        validate_audio_cohort_start(&mut session, &config, epoch)?;
    }
    let custom_policy = session
        .states
        .iter()
        .any(|state| state.gauge.profile() != &GaugeProfile::default());
    if !(2..=64).contains(&session.states.len())
        || config.origin.domain == config.stream_origin.domain
        || config.stream_origin.domain != config.playback_origin.domain
        || config.playback_origin.timestamp < config.stream_origin.timestamp
        || config.sample_rate == 0
        || config.sample_rate > 1_000_000_000
        || !(0..=1_000_000_000).contains(&config.advance_lag.as_nanos())
    {
        return Err("invalid cohort configuration".into());
    }
    for (index, state) in session.states.iter().enumerate() {
        if state.player.0 == 0
            || session.states[..index]
                .iter()
                .any(|previous| previous.player == state.player)
            || session.group.member_judge(state.player).is_none()
        {
            return Err("invalid cohort roster".into());
        }
    }
    let mut acquired = VecDeque::new();
    let mut completed_through = None;
    if custom_policy {
        if session.group.poisoned()
            || !session
                .group
                .player_ids()
                .eq(session.states.iter().map(|state| state.player))
        {
            return Err("nondefault cohort requires the exact usable group roster".into());
        }
        if session
            .network
            .as_ref()
            .is_some_and(|network| !network.policy_agnostic())
        {
            return Err(
                "nondefault cohort networking requires policy-aware member identities".into(),
            );
        }
        for state in session.states.iter() {
            if state.score != ScoreSummary::default()
                || state.last_song != config.song_origin
                || session.group.player_gameplay_fence(state.player).is_some()
            {
                return Err("nondefault cohort requires pristine member state".into());
            }
            let judge = session
                .group
                .member_judge(state.player)
                .expect("roster checked");
            crate::native_policy_admission::validate_initial(judge, &state.gauge)?;
            crate::native_policy_admission::validate_capture_in_domain(
                judge,
                state.gauge.profile(),
                state.capture.as_ref(),
                &config,
                logical_domain,
                section_start,
            )?;
            if let Some(competition) = state
                .competition
                .as_ref()
                .filter(|port| !port.policy_agnostic())
            {
                let header = competition
                    .expected_policy_header()
                    .ok_or("nondefault member competition has no policy identity")?;
                crate::native_policy_admission::validate_header_in_domain(
                    judge,
                    state.gauge.profile(),
                    header,
                    &config,
                    logical_domain,
                    section_start,
                )?;
                if state
                    .capture
                    .as_ref()
                    .is_some_and(|capture| capture.header() != header)
                {
                    return Err("member capture and competition policy identities differ".into());
                }
            }
        }
        let mut policies = Vec::new();
        policies.try_reserve_exact(session.states.len())?;
        policies.extend(
            session
                .states
                .iter()
                .map(|state| (state.player, state.gauge.profile())),
        );
        acquired.try_reserve_exact(64 * 256)?;
        host_port.prepare_policies(&policies)?;
    }
    let mut deadline =
        NativePumpDeadline::new(control, config.seconds, "cohort deadline overflow")?;
    let lag = config.advance_lag.as_nanos();
    if !custom_policy {
        acquired.try_reserve_exact(64 * 256)?;
    }
    let mut last_host: Option<ClockPoint> = None;
    let mut stop_evidence = OwnedStopEvidence::default();
    let mut stop_barrier = NativeStopBarrier::default();
    // Preserve the calibrated lower bracket before the next native observation
    // can cross a short endpoint. Adapters supply their original evidence source.
    if let Some((owner, _)) = practice.as_mut() {
        session
            .discipline
            .audio_mut()
            .expect("audio timing")
            .pin_practice_mixer_basis(owner.mixer_basis())?;
        session
            .group
            .set_audio_scope(beatkernel::audio::CommandScope(owner.generation()));
        *session.end = None;
        owner.advertise(host_port)?;
    }
    let mut pause_song_origin = config.song_origin;
    let initial_rendered = device.render_report()?;
    validate_stop_evidence(initial_rendered, &stop_evidence)?;
    let mut end_rendered =
        initial_rendered.is_some_and(|report| report.playback_end_physical_frame.is_some());
    let mut end_boundary = if let Some(end) = session.end.as_mut() {
        session.discipline.end(device, end, initial_rendered)?
    } else {
        None
    };
    let mut committed = None;
    let mut keyboard = PauseKeyboard::new();
    let mut paused_boundary: Option<LivePauseBoundary> = None;
    let mut audio_boundary: Option<AudioLivePauseBoundary> = None;
    let mut resume_boundary = None;
    let mut pause_committed = false;
    let mut pause_lag_reached = false;
    let mut pause_announced = false;
    while !host_port.cancelled() && deadline.active(control)? {
        host_port.retry_pause_publication();
        session.discipline.observe(device)?;
        let reference = session.discipline.latest_pair();
        if !T::AUDIO && reference.is_none() {
            return Err("cohort requires native clock relation".into());
        }
        let pause_now = if T::AUDIO {
            Some(device.host_now()?)
        } else {
            None
        };
        let rendered = device.render_report()?;
        validate_stop_evidence(rendered, &stop_evidence)?;
        if let Some(end) = session.end.as_mut() {
            end_rendered |=
                rendered.is_some_and(|report| report.playback_end_physical_frame.is_some());
            if let Some(boundary) = session.discipline.end(device, end, rendered)? {
                end_boundary = Some(boundary);
            }
        }
        if let Some(reference) =
            reference.filter(|_| config.logical_schedule || config.pause_supported)
        {
            let desired = (config.pause_supported
                && !end_rendered
                && (session.pause.phase() == PausePhase::Running || pause_committed)
                && resume_boundary.is_none())
            .then(|| host_port.pause_requested())
            .filter(|desired| *desired || !device.output_replacement_pending());
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
                let update = update_live_audio_pause(
                    pause,
                    evidence,
                    rendered,
                    desired,
                    pause_song_origin,
                    config.sample_rate,
                )?;
                if let Some(boundary) = update.boundary {
                    staged_audio_boundary = Some(boundary);
                }
                crate::live_pause::LivePauseUpdate {
                    requested: update.requested,
                    boundary: update.boundary.map(|boundary| boundary.original),
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
                session.group.request_audio_pause(desired);
                host_port.publish_pause(if desired {
                    PauseState::Pausing
                } else {
                    PauseState::Resuming
                });
            }
            if let Some(boundary) = update.boundary {
                host_port.diagnostic(NativeGameplayDiagnostic::Pause {
                    local: true,
                    boundary,
                });
                let last_song = session
                    .states
                    .iter()
                    .map(|state| state.last_song)
                    .max()
                    .ok_or("cohort pause has no member prefix")?;
                if !boundary.paused
                    && session
                        .states
                        .iter()
                        .any(|state| state.last_song != boundary.song)
                {
                    return Err("cohort resume requires every member at the frozen song".into());
                }
                if boundary.paused {
                    if !end_rendered {
                        if !T::AUDIO {
                            let transport = prepare_live_transport(
                                session.group.transport_mut(),
                                boundary,
                                last_song,
                            )?;
                            *session.group.transport_mut() = transport;
                        }
                        paused_boundary = Some(boundary);
                        pause_committed = false;
                        pause_lag_reached = false;
                    }
                } else {
                    if !T::AUDIO {
                        let transport = prepare_live_transport(
                            session.group.transport_mut(),
                            boundary,
                            last_song,
                        )?;
                        session
                            .discipline
                            .resume(device, config, session.pause, reference)?;
                        *session.group.transport_mut() = transport;
                    }
                    resume_boundary = Some(boundary.at);
                    paused_boundary = None;
                    pause_committed = false;
                }
            }
        }
        crate::native_audio::feed_rendered(session.bgm, device.render_report()?, |command| {
            session.group.enqueue_audio(command)
        })?;
        // Acquire/admit during acknowledgement waits as well; otherwise native
        // loss and cross-device ordering would be hidden behind a pause request.
        let batch = device.acquire(&mut acquired)?;
        if batch.closed {
            return Ok(None);
        }
        if acquired.len() > MAX_PENDING_INPUT_EVENTS {
            return Err("cohort native batch exceeds bound".into());
        }
        let now = device.host_now()?;
        if now.domain != config.origin.domain
            || last_host.is_some_and(|previous| now.timestamp < previous.timestamp)
        {
            return Err("cohort host clock changed/regressed".into());
        }
        last_host = Some(now);
        crate::native_gameplay::validate_audio_availability(
            device,
            session.discipline,
            now,
            config.origin,
        )?;
        let cut = crate::native_gameplay::completed_cut(
            batch.completed_through,
            now,
            config.origin,
            &mut completed_through,
        )?;
        let frozen_pause = paused_boundary
            .filter(|_| {
                pause_committed && session.pause.phase() == PausePhase::Paused && !end_rendered
            })
            .map(|boundary| boundary.at);
        crate::gameplay_presentation::validate_pump_host(
            device,
            session.discipline,
            now,
            frozen_pause,
        )?;
        while let Some(event) = acquired.pop_front() {
            let host = point(&event);
            if host.domain != config.origin.domain || host.timestamp > now.timestamp {
                return Err("cohort input has invalid domain/future timestamp".into());
            }
            if host.timestamp < config.origin.timestamp {
                *session.pre_origin_inputs = session
                    .pre_origin_inputs
                    .checked_add(1)
                    .ok_or("cohort pre-origin counter overflow")?;
                continue;
            }
            crate::gameplay_presentation::validate_pump_host(
                device,
                session.discipline,
                host,
                frozen_pause,
            )?;
            session.delivery.observe(host, now)?;
            session.merger.admit(event, now)?;
        }
        if T::AUDIO {
            if let Some(prefix) = cut
                .map(|cut| session.merger.watermark(cut, lag, batch.backlog))
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
        if now.timestamp < config.origin.timestamp || batch.backlog {
            control.wait(WallDuration::from_millis(1))?;
            continue;
        }
        // A rendered receipt fences the old attempt immediately. Qualification
        // uses native presentation before any event can cross the original cut.
        let practice_boundary = if let Some((owner, _)) = practice.as_mut() {
            if session.pause.phase() == PausePhase::Running {
                if let Some(report) = rendered {
                    owner.service_request(
                        host_port,
                        report
                            .playback_start_frame
                            .checked_add(u64::try_from(report.playback_frames)?)
                            .ok_or("practice playback cursor overflow")?,
                    )?;
                }
            }
            let held = owner.peek_receipt()?.is_some();
            let qualified =
                owner.qualify(session.discipline.audio().expect("audio timing"), now)?;
            if (held || (owner.ended() && !owner.terminal_ready())) && qualified.is_none() {
                control.wait(WallDuration::from_millis(1))?;
                continue;
            }
            qualified
        } else {
            None
        };
        if T::AUDIO {
            let practice_end = practice.as_ref().and_then(|_| config.end_song);
            service_audio_cohort(
                device,
                &mut session,
                &mut config,
                now,
                batch.backlog,
                &mut keyboard,
                &mut paused_boundary,
                &mut audio_boundary,
                &mut pause_committed,
                &mut committed,
                end_rendered,
                end_boundary,
                &mut stop_evidence,
                host_port,
                practice_boundary.map(|boundary| boundary.host()),
                practice_end,
            )?;
            if let Some(boundary) = practice_boundary {
                if !session
                    .discipline
                    .audio()
                    .expect("audio timing")
                    .practice_boundary_ready(&boundary, now, session.merger)?
                {
                    control.wait(WallDuration::from_millis(1))?;
                    continue;
                }
                let (owner, recording) = practice.as_mut().expect("practice boundary owner");
                commit_cohort_practice(
                    device,
                    &mut session,
                    &mut config,
                    owner,
                    *recording,
                    boundary,
                    now,
                    &mut stop_evidence,
                    host_port,
                )?;
                committed = Some(boundary.host());
                if matches!(
                    boundary.receipt().kind,
                    beatkernel::audio::PracticeBoundaryKind::Requested
                        | beatkernel::audio::PracticeBoundaryKind::Looped
                ) {
                    let elapsed = i128::from(boundary.receipt().playback_frame) * 1_000_000_000
                        / i128::from(config.sample_rate);
                    pause_song_origin = Timestamp::from_nanos(i64::try_from(
                        i128::from(boundary.receipt().applied_song_time.as_nanos()) - elapsed,
                    )?);
                    keyboard = PauseKeyboard::new();
                    paused_boundary = None;
                    audio_boundary = None;
                    resume_boundary = None;
                    pause_committed = false;
                    pause_lag_reached = false;
                    stop_barrier = NativeStopBarrier::default();
                    end_boundary = None;
                    end_rendered = false;
                }
                if owner.terminal_ready() {
                    return complete_cohort(&mut session, config, host_port);
                }
                // Drain each receipt in order; do not advance into a later pass.
                continue;
            }
            resume_boundary = audio_boundary
                .filter(|boundary| !boundary.original.paused)
                .map(|boundary| boundary.original.at);
            if session.pause.phase() != PausePhase::Running || audio_boundary.is_some() {
                control.wait(WallDuration::from_millis(1))?;
                continue;
            }
        } else {
            let Some(cut) = cut else {
                control.wait(WallDuration::from_millis(1))?;
                continue;
            };
            if session.pause.phase() == PausePhase::Paused && !end_rendered {
                let boundary = paused_boundary.ok_or("cohort paused boundary unavailable")?;
                let at = boundary.at;
                if !pause_committed && lag_reaches(cut, at, lag)? {
                    while let Some(event) = session.merger.pop_ready(at)? {
                        if point(&event).timestamp >= at.timestamp {
                            keyboard.observe_paused(event)?;
                        } else if keyboard.accept(&event)? {
                            validate_pre_pause_input(
                                session.group.transport_mut(),
                                boundary,
                                point(&event),
                            )?;
                            process_with_host(
                                device,
                                &mut session,
                                config,
                                event,
                                &mut stop_evidence,
                                host_port,
                            )?;
                        }
                    }
                    advance(
                        device,
                        &mut session,
                        config,
                        at,
                        &mut stop_evidence,
                        host_port,
                    )?;
                    session.merger.commit(at)?;
                    committed = Some(at);
                    pause_committed = true;
                    host_port.publish_pause(PauseState::Paused);
                }
                if pause_committed {
                    if !pause_lag_reached {
                        pause_lag_reached = lag_reaches(cut, at, lag)?;
                    }
                    if pause_lag_reached {
                        if let Some(frontier) = session.merger.watermark(cut, lag, false)? {
                            while let Some(event) = session.merger.pop_ready(frontier)? {
                                keyboard.observe_paused(event)?;
                            }
                        }
                    }
                }
                if pause_committed
                    && end_boundary.is_none()
                    && resume_boundary.is_none()
                    && !host_port.cancelled()
                {
                    session.discipline.publish(
                        device,
                        crate::gameplay_presentation::GameplayPauseControl::cohort(session.group),
                        None,
                        session.pause,
                        &mut config,
                        session.end,
                        now,
                    )?;
                }
                control.wait(WallDuration::from_millis(1))?;
                continue;
            }
            if let Some(at) = resume_boundary {
                if !lag_reaches(cut, at, lag)? {
                    control.wait(WallDuration::from_millis(1))?;
                    continue;
                }
            }
            let frontier = session.merger.watermark(cut, lag, false)?;
            if resume_boundary
                .is_some_and(|at| frontier.is_none_or(|frontier| frontier.timestamp < at.timestamp))
            {
                control.wait(WallDuration::from_millis(1))?;
                continue;
            }
            let was_resuming = resume_boundary.is_some();
            if !was_resuming {
                session
                    .discipline
                    .correction(now, session.group.transport_mut())?;
            }
            if let Some(frontier) = frontier {
                while let Some(event) = session.merger.pop_ready(frontier)? {
                    let host = point(&event);
                    session.discipline.validate_host(host)?;
                    if let Some(at) = resume_boundary {
                        if host.timestamp < at.timestamp {
                            keyboard.observe_paused(event)?;
                            continue;
                        }
                        reconcile(
                            device,
                            &mut session,
                            config,
                            &mut keyboard,
                            at,
                            &mut stop_evidence,
                            host_port,
                        )?;
                        resume_boundary = None;
                        host_port.publish_pause(PauseState::Running);
                    }
                    if end_boundary
                        .is_some_and(|boundary| host.timestamp >= boundary.host.timestamp)
                    {
                        continue;
                    }
                    if !config.pause_supported || keyboard.accept(&event)? {
                        process_with_host(
                            device,
                            &mut session,
                            config,
                            event,
                            &mut stop_evidence,
                            host_port,
                        )?;
                    }
                }
                if let Some(at) = resume_boundary.take() {
                    reconcile(
                        device,
                        &mut session,
                        config,
                        &mut keyboard,
                        at,
                        &mut stop_evidence,
                        host_port,
                    )?;
                    host_port.publish_pause(PauseState::Running);
                }
                if was_resuming {
                    session
                        .discipline
                        .correction(now, session.group.transport_mut())?;
                }
                advance(
                    device,
                    &mut session,
                    config,
                    frontier,
                    &mut stop_evidence,
                    host_port,
                )?;
                session.merger.commit(frontier)?;
                committed = Some(frontier);
            }
        }
        let rendered = device.render_report()?;
        validate_stop_evidence(rendered, &stop_evidence)?;
        let stops_rendered = if config.end_song.is_some() {
            stop_barrier.observe(&stop_evidence, rendered)?
        } else {
            true
        };
        let acquired_ready = cut.zip(committed).is_some_and(|(cut, committed)| {
            !batch.backlog
                && i128::from(cut.timestamp.as_nanos()) - i128::from(lag)
                    >= i128::from(committed.timestamp.as_nanos())
        });
        if practice.is_none()
            && stops_rendered
            && acquired_ready
            && finite_cohort_done_with_terminal(
                config.end_song.map(|end| end.as_nanos()),
                end_boundary.map(|boundary| boundary.host),
                committed,
                session.states,
                session.group,
                false,
                resume_boundary.is_some(),
                session.bgm.report(),
                rendered,
            )
        {
            return complete_cohort(&mut session, config, host_port);
        }
        let natural_practice = practice.as_ref().is_some_and(|(owner, _)| {
            owner.can_complete_naturally()
                && session
                    .states
                    .iter()
                    .all(|state| state.last_song >= owner.natural_completion_floor())
        });
        if (practice.is_none() || natural_practice)
            && config.end_song.is_none()
            && acquired_ready
            && session.pause.phase() == PausePhase::Running
            && resume_boundary.is_none()
            && session.merger.pending() == 0
        {
            let mut finished = true;
            for state in session.states.iter_mut() {
                let Some(completion) = state.completion.as_mut() else {
                    finished = false;
                    continue;
                };
                let judge = session
                    .group
                    .member_judge(state.player)
                    .ok_or("cohort judge unavailable")?;
                let presented = session.discipline.latest_pair().map(|pair| pair.source);
                let numeric_terminal = state.gauge.snapshot().failure.is_some()
                    && session.group.player_gameplay_fence(state.player).is_some();
                finished &= if numeric_terminal {
                    completion.observe_terminal_ready(
                        true,
                        session.bgm.report(),
                        rendered,
                        presented,
                    )?
                } else {
                    completion.observe(
                        judge,
                        state.last_song,
                        session.bgm.report(),
                        rendered,
                        presented,
                    )?
                };
            }
            if finished {
                return complete_cohort(&mut session, config, host_port);
            }
        }
        control.wait(WallDuration::from_millis(1))?;
    }
    Ok(None)
}

#[cfg(test)]
mod fixtures {
    mod competition_port {
        include!("native_local_competition_port_fixtures.rs");
    }
    use crate::player;
    mod host {
        include!("native_local_host_fixtures.rs");
    }
    mod control {
        include!("native_local_control_fixtures.rs");
    }
    mod failed_terminal {
        include!("native_local_failed_terminal_fixtures.rs");
    }
    mod sound_stop {
        include!("native_local_sound_stop_fixtures.rs");
    }
    mod gauge_fence {
        include!("native_local_gauge_fence_fixtures.rs");
    }
    mod policy_admission {
        use super::*;
        include!("native_cohort_policy_admission_fixtures.rs");
    }
    include!("native_cohort_interval_fixtures.rs");
    use super::*;
    use crate::local_runtime::MemberConfig;
    use beatkernel::{
        audio::*,
        input::{
            Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
            EventMeta, GameControlId, PhysicalControlId,
        },
        judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
        replay::{codec::ReplayCodecLimits, ReplayOperation},
        runtime::SoundBinding,
        time::Duration,
        transport::{Rate, Transport},
    };
    fn host(ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(1),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn output(ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn button(device: u64, ns: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
        PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(device), host(ns), sequence),
            control: PhysicalControlId::keyboard(4),
            state,
        })
    }
    fn limits() -> ReplayCodecLimits {
        ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 4096).unwrap()).unwrap()
    }
    struct Device {
        mixer: Mixer,
        report: Option<RenderReport>,
        step: u64,
        pcm: Vec<f32>,
        pause_flow: bool,
        viewer: Option<player::PlayerViewer>,
        seeded: usize,
    }
    impl NativeGameplayDevice for Device {
        fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
            let mut pcm = [0.0; 10];
            self.report = Some(self.mixer.render(&mut pcm)?);
            self.pcm.extend_from_slice(&pcm);
            self.step += 1;
            discipline.observe_clock_pair(ClockPair {
                source: output(self.step as i64 * 10_000_000),
                target: host(self.step as i64 * 10_000_000),
            })?;
            Ok(())
        }
        fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
            Ok(self.report)
        }
        fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
            Ok(host(self.step as i64 * 10_000_000))
        }
        fn acquire(
            &mut self,
            events: &mut VecDeque<PhysicalInputEvent>,
        ) -> NativeGameplayResult<crate::native_gameplay::InputBatch> {
            use crate::native_gameplay::{retain_input, InputBatch};
            if self.pause_flow {
                match self.step {
                    1 => {
                        for device in [2, 1] {
                            retain_input(events, button(device, 10_000_000, 1, ButtonState::Down))?;
                        }
                        self.viewer.as_ref().unwrap().request_pause(true);
                    }
                    2 => {
                        for device in [2, 1] {
                            retain_input(events, button(device, 20_000_000, 2, ButtonState::Up))?;
                        }
                    }
                    3 => self.viewer.as_ref().unwrap().request_pause(false),
                    5 => {
                        for device in [2, 1] {
                            retain_input(events, button(device, 40_000_000, 3, ButtonState::Down))?;
                        }
                    }
                    _ => {}
                }
            } else if self.step == 2 {
                for device in [2, 1] {
                    retain_input(events, button(device, 20_000_000, 1, ButtonState::Down))?;
                }
            }
            Ok(InputBatch {
                completed_through: Some(host(self.step as i64 * 10_000_000)),
                backlog: if self.pause_flow {
                    self.step == 5
                } else {
                    self.step == 2
                },
                closed: self.step >= if self.pause_flow { 7 } else { 4 },
            })
        }
        fn observe_end(
            &mut self,
            end: &mut NativeEnd,
            discipline: &PresentationDiscipline,
            report: Option<RenderReport>,
        ) -> NativeGameplayResult<Option<crate::native_end::EndBoundary>> {
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
            Ok(output(self.step as i64 * 10_000_000))
        }
    }
    use beatkernel::time::ClockPair;
    struct Fixture {
        source: beatkernel_bms::BmsChart,
        device: Device,
        group: RuntimeGroup,
        states: Vec<PlayerState>,
        merger: InputMerger,
        bgm: BgmFeeder,
        discipline: PresentationDiscipline,
        pause: NativePause,
        end: Option<NativeEnd>,
        delivery: InputDeliveryTelemetry,
        pre: u64,
    }
    impl Fixture {
        fn register_presentation(&self) -> Result<(), String> {
            let compiled = self.source.compile().map_err(|error| error.to_string())?;
            let players: Vec<_> = self.states.iter().map(|state| state.player).collect();
            player::publish_local_chart(&self.source, &compiled.chart, &players)
                .map_err(|error| error.to_string())
        }
        fn new(finite: bool, capacity: usize) -> Self {
            let source = beatkernel_bms::parse(
                "#BPM 3000\n#WAV01 original.wav\n#00011:00010000\n",
                Default::default(),
            )
            .unwrap();
            let compiled = source.compile().unwrap();
            let mut configs = Vec::new();
            let mut states = Vec::new();
            for (device, player) in [(1, PlayerId(7)), (2, PlayerId(u32::MAX))] {
                let judge = JudgeEngine::new(
                    compiled.chart.clone(),
                    source.rules(),
                    JudgeProfile::new(
                        vec![JudgeWindow {
                            grade: JudgeGrade(1),
                            early: Duration::from_nanos(10_000_000),
                            late: Duration::from_nanos(10_000_000),
                        }],
                        Duration::ZERO,
                    )
                    .unwrap(),
                )
                .unwrap();
                let capture =
                    Some(LiveReplayCapture::new(&judge, ClockDomainId(1), limits()).unwrap());
                states.push(PlayerState {
                    player,
                    capture,
                    competition: None,
                    completion: None,
                    score: ScoreSummary::default(),
                    gauge: BmsGauge::default(),
                    last_song: Timestamp::ZERO,
                });
                configs.push(MemberConfig {
                    player,
                    device: Some(DeviceId(device)),
                    bindings: BindingMap::from_bindings([Binding {
                        device: DeviceSelector::Exact(DeviceId(device)),
                        physical: PhysicalControlId::keyboard(4),
                        game_control: GameControlId(0x11),
                    }])
                    .unwrap(),
                    judge,
                    sounds: vec![SoundBinding {
                        object: compiled.chart.objects()[0].id,
                        stage: beatkernel::judge::JudgeStage::Instant,
                        sample: SampleId(1),
                        voice: VoiceId(device),
                        gain: 1.0,
                    }],
                });
            }
            let format = AudioFormat::new(1000, 1).unwrap();
            let pcm_limits = PcmLimits::new(64, 256, 1).unwrap();
            let mut bank = SampleBank::new(format, pcm_limits).unwrap();
            bank.insert(
                SampleId(1),
                PcmSample::new(format, vec![0.25, 0.5], pcm_limits).unwrap(),
            )
            .unwrap();
            let (producer, consumer) = command_queue(capacity).unwrap();
            let config = MixerConfig::new(
                format,
                ClockDomainId(2),
                Timestamp::ZERO,
                AudioLimits::new(capacity, 2, 8, 16, 8).unwrap(),
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
            let mut group = RuntimeGroup::new(
                ClockDomainId(1),
                ClockDomainId(2),
                Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
                producer,
                configs,
                8,
                &[],
            )
            .unwrap();
            if finite {
                group
                    .set_song_end(Timestamp::from_nanos(10_000_000))
                    .unwrap();
            }
            let initial = ClockPair {
                source: output(0),
                target: host(0),
            };
            let mut discipline = PresentationDiscipline::new(
                DisciplineConfig::default(),
                output(0),
                ClockDomainId(1),
                Timestamp::ZERO,
            )
            .unwrap();
            discipline.observe_clock_pair(initial).unwrap();
            let mut pause = NativePause::new(output(0), ClockDomainId(1), 1000).unwrap();
            let end = if finite {
                pause = pause.with_playback_end_frame(10).unwrap();
                let mut end = NativeEnd::new(output(0), ClockDomainId(1), 1000, 10).unwrap();
                end.observe(None, initial).unwrap();
                Some(end)
            } else {
                None
            };
            Self {
                source,
                device: Device {
                    mixer,
                    report: None,
                    step: 0,
                    pcm: Vec::new(),
                    pause_flow: false,
                    viewer: None,
                    seeded: 0,
                },
                group,
                states,
                merger: InputMerger::new(
                    ClockDomainId(1),
                    host(0),
                    vec![DeviceId(1), DeviceId(2)],
                    16,
                )
                .unwrap(),
                bgm: BgmFeeder::new(
                    Vec::new(),
                    crate::bgm::BgmConfig {
                        output_origin: output(0),
                        sample_rate: 1000,
                        preroll: Duration::ZERO,
                        lookahead: Duration::from_nanos(1_000_000_000),
                        max_pending: 4,
                    },
                )
                .unwrap(),
                discipline,
                pause,
                end,
                delivery: InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap(),
                pre: 0,
            }
        }
        fn run(&mut self, finite: bool) -> NativeGameplayResult<()> {
            let pause_supported = self.device.pause_flow;
            run_cohort(
                &mut self.device,
                NativeCohortSession {
                    network: None,
                    group: &mut self.group,
                    states: &mut self.states,
                    merger: &mut self.merger,
                    bgm: &mut self.bgm,
                    discipline: &mut self.discipline,
                    pause: &mut self.pause,
                    end: &mut self.end,
                    delivery: &mut self.delivery,
                    pre_origin_inputs: &mut self.pre,
                },
                NativeGameplayConfig {
                    origin: host(0),
                    stream_origin: output(0),
                    playback_origin: output(0),
                    song_origin: Timestamp::ZERO,
                    sample_rate: 1000,
                    end_song: finite.then_some(Timestamp::from_nanos(10_000_000)),
                    advance_lag: Duration::from_nanos(10_000_000),
                    seconds: None,
                    pause_supported,
                    logical_schedule: true,
                },
            )
        }
    }
    #[test]
    fn real_cohort_equal_time_backlog_preserves_each_capture_and_actual_pcm() {
        let mut fixture = Fixture::new(false, 8);
        fixture.run(false).unwrap();
        assert_eq!(&fixture.device.pcm[30..33], &[0.5, 1.0, 0.0]);
        assert_eq!(fixture.delivery.observed_events(), 2);
        for state in &fixture.states {
            assert_eq!(state.score.hits, 1);
            let capture = state.capture.as_ref().unwrap();
            assert_eq!(
                capture
                    .records()
                    .iter()
                    .map(|record| record.song_time.as_nanos())
                    .collect::<Vec<_>>(),
                vec![0, 20_000_000, 20_000_000]
            );
            crate::replay_playback::reconstruct(
                &fixture.source,
                beatkernel::replay::codec::ReplayFile::new(
                    capture.header().clone(),
                    capture.records().to_vec(),
                ),
                limits(),
            )
            .unwrap();
        }
        assert_eq!(fixture.merger.pending(), 0);
        // Missing completion cannot be interpreted as an already finished song.
        assert_eq!(fixture.device.step, 4);
    }
    #[test]
    fn acquisition_during_ack_wait_and_resume_reconciliation_keep_original_prefix() {
        let (publisher, viewer) = player::channel();
        let mut fixture = Fixture::new(false, 8);
        fixture.device.pause_flow = true;
        fixture.device.viewer = Some(viewer);
        player::with_publisher(publisher, || {
            fixture.register_presentation()?;
            fixture.run(false).map_err(|error| error.to_string())
        })
        .unwrap();
        assert_eq!(fixture.device.seeded, 1);
        assert_eq!(fixture.delivery.observed_events(), 6);
        for state in &fixture.states {
            let releases = state
                .capture
                .as_ref()
                .unwrap()
                .records()
                .iter()
                .filter_map(|record| match &record.operation {
                    ReplayOperation::Input(input) => match &input.physical {
                        PhysicalInputEvent::Button(button) if button.state == ButtonState::Up => {
                            Some((button.meta.timestamp, button.meta.original_clock_point))
                        }
                        _ => None,
                    },
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                releases,
                vec![(host(40_000_000).timestamp, Some(host(20_000_000)))]
            );
        }
    }
    #[test]
    fn finite_cohort_commits_all_logical_prefixes_only_after_native_and_input_drain() {
        let mut fixture = Fixture::new(true, 8);
        fixture.run(true).unwrap();
        assert_eq!(fixture.device.step, 3);
        assert_eq!(fixture.delivery.observed_events(), 2);
        assert!(fixture
            .states
            .iter()
            .all(|state| state.last_song == Timestamp::from_nanos(10_000_000)
                && state.score.hits == 0));
        assert_eq!(fixture.merger.pending(), 0);
    }
    #[test]
    fn actual_partial_audio_failure_captures_every_committed_report_before_error() {
        let mut fixture = Fixture::new(false, 1);
        assert!(fixture.run(false).is_err());
        assert!(fixture.group.poisoned());
        assert!(fixture.states.iter().all(|state| state.score.hits == 1));
        assert!(fixture.states.iter().all(|state| {
            state
                .capture
                .as_ref()
                .unwrap()
                .records()
                .iter()
                .any(|record| matches!(record.operation, ReplayOperation::Input(_)))
        }));
    }
    #[test]
    fn deterministic_merger_order_and_lag_guard_do_not_mask_regression() {
        let mut merger =
            InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(1), DeviceId(2)], 4).unwrap();
        for device in [2, 1] {
            merger
                .admit(button(device, 9, 1, ButtonState::Down), host(20))
                .unwrap();
        }
        assert!(merger.watermark(host(20), 2, true).unwrap().is_none());
        assert_eq!(
            merger.pop_ready(host(10)).unwrap().unwrap().meta().source,
            DeviceId(1)
        );
        assert_eq!(
            merger.pop_ready(host(10)).unwrap().unwrap().meta().source,
            DeviceId(2)
        );
        merger.commit(host(10)).unwrap();
        assert!(!lag_reaches(host(11), host(10), 2).unwrap());
        assert!(lag_reaches(host(12), host(10), 2).unwrap());
        assert!(merger.watermark(host(11), 2, false).is_err());
        assert!(lag_reaches(output(10), host(10), 0).is_err());
        assert!(!lag_reaches(host(i64::MIN), host(i64::MIN), 1).unwrap());
    }
    #[test]
    fn sparse_identity_replay_paths_and_finite_all_member_requirement() {
        let mut fixture = Fixture::new(true, 8);
        for state in &mut fixture.states {
            state.last_song = Timestamp::from_nanos(10);
        }
        assert!(finite_cohort_done(
            Some(10),
            Some(host(20)),
            Some(host(20)),
            &fixture.states,
            false,
            false
        ));
        assert!(!finite_cohort_done(
            Some(10),
            Some(host(20)),
            None,
            &fixture.states,
            false,
            false
        ));
        assert!(!finite_cohort_done(
            Some(10),
            Some(host(20)),
            Some(host(20)),
            &fixture.states,
            true,
            false
        ));
        fixture.states[1].last_song = Timestamp::from_nanos(9);
        assert!(!finite_cohort_done(
            Some(10),
            Some(host(20)),
            Some(host(20)),
            &fixture.states,
            false,
            false
        ));
        assert_eq!(
            replay_path(Path::new("records/run.bkr"), PlayerId(u32::MAX)).unwrap(),
            PathBuf::from("records/run.p4294967295.bkr")
        );
        assert!(replay_path(Path::new("run.bkr"), PlayerId(0)).is_err());
    }
}
/// Cohort session using original native evidence and one shared audio authority.
pub type AudioCohortSession<'a, S, G> =
    CohortSession<'a, S, G, crate::native_audio_presentation::NativeAudioPresentation>;

fn validate_audio_cohort_start<S: SoloCompetitionPort, G: GroupCompetitionPort, P>(
    session: &mut CohortSession<'_, S, G, P>,
    config: &NativeGameplayConfig,
    epoch: crate::audio_authority::AudioAuthorityEpoch,
) -> NativeGameplayResult<()> {
    if session.group.clock_domains()
        != Some((epoch.logical_origin.domain, epoch.stream_origin.domain))
        || epoch.host_domain != config.origin.domain
        || epoch.stream_origin != config.stream_origin
        || session.group.transport_mut().anchor().host_time
            != epoch.logical_output(config.playback_origin)?.timestamp
        || session.group.transport_mut().anchor().song_time != config.song_origin
        || session.group.transport_mut().anchor().rate != beatkernel::transport::Rate::NORMAL
        || session.group.poisoned()
        || session
            .states
            .iter()
            .any(|state| session.group.player_gameplay_fence(state.player).is_some())
    {
        return Err("audio cohort requires pristine actual logical/raw domains and origin".into());
    }
    Ok(())
}

fn observe_audio_group_result<
    S: SoloCompetitionPort,
    G: GroupCompetitionPort,
    T,
    H: NativeGameplayHost,
>(
    result: Result<Vec<PlayerReport>, GroupError>,
    session: &mut CohortSession<'_, S, G, T>,
    commit: impl FnOnce(&mut T, &mut InputMerger),
    stops: &mut OwnedStopEvidence,
    host: &mut H,
) -> NativeGameplayResult<()> {
    match result {
        Ok(mut reports) => {
            commit(session.discipline, session.merger);
            observe_reports_with_competition(
                &mut reports,
                session.states,
                session.group,
                session.network.as_deref_mut(),
                stops,
                host,
            )
        }
        Err(mut error) => {
            if !error.completed_reports.is_empty() {
                commit(session.discipline, session.merger);
            }
            let observation_error = observe_reports_with_competition(
                &mut error.completed_reports,
                session.states,
                session.group,
                session.network.as_deref_mut(),
                stops,
                host,
            )
            .err();
            Err(Box::new(NativeCohortProcessingError {
                group_error: error,
                observation_error,
            }))
        }
    }
}

fn service_audio_cohort<
    D: crate::gameplay_presentation::GameplayDevice,
    S: SoloCompetitionPort,
    G: GroupCompetitionPort,
    H: NativeGameplayHost,
    T: PumpTiming<D>,
>(
    device: &mut D,
    session: &mut CohortSession<'_, S, G, T>,
    config: &mut NativeGameplayConfig,
    now: ClockPoint,
    backlog: bool,
    keyboard: &mut PauseKeyboard,
    paused: &mut Option<LivePauseBoundary>,
    transition: &mut Option<AudioLivePauseBoundary>,
    pause_committed: &mut bool,
    committed: &mut Option<ClockPoint>,
    end_rendered: bool,
    end_boundary: Option<crate::native_end::EndBoundary>,
    stops: &mut OwnedStopEvidence,
    host: &mut H,
    practice_cut: Option<ClockPoint>,
    practice_end: Option<Timestamp>,
) -> NativeGameplayResult<()> {
    let Some(prefix) = session
        .discipline
        .audio()
        .expect("audio timing")
        .authority()
        .acquired_prefix()
    else {
        return Ok(());
    };
    loop {
        let Some(event) = session.merger.peek_ready(prefix)? else {
            break;
        };
        let at = point(event);
        if practice_cut.is_some_and(|cut| at.timestamp >= cut.timestamp) {
            break;
        }
        if transition.is_some_and(|boundary| {
            !boundary.original.paused && at.timestamp >= boundary.original.at.timestamp
        }) {
            break;
        }
        if paused.is_some_and(|boundary| at.timestamp >= boundary.at.timestamp) {
            keyboard.observe_paused(
                session
                    .merger
                    .pop_ready(prefix)?
                    .expect("peeked paused input"),
            )?;
            continue;
        }
        if end_boundary.is_some_and(|boundary| at.timestamp >= boundary.host.timestamp) {
            session.merger.pop_ready(prefix)?;
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
        // The render block can end exactly on the next pass boundary before
        // the audio owner emits its next-block receipt. Keep equal/later input
        // queued until that actual receipt authorizes the new attempt. Once
        // qualified, drain the subframe gap through Runtime's finite end rule.
        if let Some(end) = practice_end.filter(|_| practice_cut.is_none()) {
            if session
                .group
                .transport_mut()
                .position_at(prepared.output().timestamp)?
                >= end
            {
                break;
            }
        }
        if config.pause_supported && !keyboard.accept(event)? {
            session.merger.pop_ready(prefix)?;
            continue;
        }
        let audio_at = schedule(device, session, *config)?;
        if audio_at.domain != config.stream_origin.domain {
            return Err("cohort audio scheduling domain differs".into());
        }
        let event = session
            .merger
            .pop_ready(prefix)?
            .expect("prepared earliest original input");
        let result = match session
            .group
            .process_input(event, prepared.mapper(), audio_at)
        {
            Ok(InputResult::Processed(reports)) => Ok(reports),
            Ok(InputResult::Ignored { device }) => {
                return Err(format!("merged source {device:?} has no cohort owner").into());
            }
            Err(error) => Err(error),
        };
        observe_audio_group_result(
            result,
            session,
            |timing, _| {
                timing
                    .audio_mut()
                    .expect("audio timing")
                    .authority_mut()
                    .commit_input(prepared)
                    .expect("private current group input token was preflighted");
            },
            stops,
            host,
        )?;
    }
    if practice_cut.is_some() {
        return Ok(());
    }
    if !backlog {
        if let Some(boundary) = *transition {
            let authority = session
                .discipline
                .audio()
                .expect("audio timing")
                .authority();
            let prepared = if boundary.original.paused {
                authority.prepare_control_cutoff(
                    boundary.epoch,
                    boundary.raw_output,
                    boundary.original.at,
                    now,
                    session.merger,
                )?
            } else {
                authority.prepare_resume_control_cutoff(
                    boundary.epoch,
                    boundary.raw_output,
                    boundary.original.at,
                    now,
                    session.merger,
                )?
            };
            if let Some(prepared) = prepared {
                let last_song = session
                    .states
                    .iter()
                    .map(|state| state.last_song)
                    .max()
                    .ok_or("audio cohort has no member prefix")?;
                let transport = prepare_live_audio_transport(
                    session.group.transport_mut(),
                    boundary,
                    &prepared,
                    last_song,
                )?;
                let audio_at = schedule(device, session, *config)?;
                *session.group.transport_mut() = transport;
                let result =
                    session
                        .group
                        .advance_to(prepared.output(), &ExplicitDomains, audio_at);
                observe_audio_group_result(
                    result,
                    session,
                    |timing, merger| {
                        timing
                            .audio_mut()
                            .expect("audio timing")
                            .authority_mut()
                            .commit_control_cutoff(prepared, merger)
                            .expect("private current group control token was preflighted");
                        *committed = Some(boundary.original.at);
                    },
                    stops,
                    host,
                )?;
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
                                session.merger,
                            )?
                            .ok_or("accepted cohort resume lost authorized control mapping")?;
                        let mapper = crate::native_gameplay::AudioControlMapper(prepared);
                        let result = match session.group.process_input(event, &mapper, audio_at) {
                            Ok(InputResult::Processed(reports)) => Ok(reports),
                            Ok(InputResult::Ignored { device }) => {
                                return Err(format!(
                                    "resume source {device:?} has no cohort owner"
                                )
                                .into());
                            }
                            Err(error) => Err(error),
                        };
                        observe_audio_group_result(
                            result,
                            session,
                            |timing, merger| {
                                timing
                                    .audio_mut()
                                    .expect("audio timing")
                                    .authority_mut()
                                    .commit_control_cutoff(prepared, merger)
                                    .expect("private reconciliation token remains current");
                            },
                            stops,
                            host,
                        )?;
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
                .prepare_held_frontier(now, session.merger)?
            {
                *committed = Some(frontier.host());
                session
                    .discipline
                    .audio_mut()
                    .expect("audio timing")
                    .authority_mut()
                    .commit_held_frontier(frontier, session.merger)
                    .expect("private group held frontier remains current");
            }
        }
        if end_boundary.is_none() && !host.cancelled() {
            session.discipline.publish(
                device,
                crate::gameplay_presentation::GameplayPauseControl::cohort(session.group),
                Some(session.merger),
                session.pause,
                config,
                session.end,
                now,
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
    if let Some(frontier) = session
        .discipline
        .audio()
        .expect("audio timing")
        .authority()
        .prepare_frontier(now, session.merger)?
    {
        if let Some(output) = frontier.advance() {
            if let Some(end) = practice_end {
                if session
                    .group
                    .transport_mut()
                    .position_at(output.timestamp)?
                    >= end
                {
                    return Ok(());
                }
            }
            let audio_at = schedule(device, session, *config)?;
            let result = session.group.advance_to(output, &ExplicitDomains, audio_at);
            observe_audio_group_result(
                result,
                session,
                |timing, merger| {
                    timing
                        .audio_mut()
                        .expect("audio timing")
                        .authority_mut()
                        .commit_frontier(frontier, merger)
                        .expect("private current group frontier was preflighted");
                    *committed = Some(frontier.host());
                },
                stops,
                host,
            )?;
        } else {
            session
                .discipline
                .audio_mut()
                .expect("audio timing")
                .authority_mut()
                .commit_frontier(frontier, session.merger)
                .expect("private group closure frontier remains current");
            *committed = Some(frontier.host());
        }
    }
    Ok(())
}

pub fn run_cohort_audio_with_results_and_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
    G: GroupCompetitionPort,
>(
    device: &mut D,
    session: AudioCohortSession<'_, S, G>,
    config: AudioGameplayConfig,
    control: &mut C,
    host: &mut H,
) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
    crate::native_policy_admission::validate_audio_config(&config)?;
    let section_start = config.section_start;
    let config = config.gameplay;
    let mut timing = AudioTiming(session.discipline, section_start);
    run_cohort_timed(
        device,
        CohortSession {
            group: session.group,
            network: session.network,
            states: session.states,
            merger: session.merger,
            bgm: session.bgm,
            discipline: &mut timing,
            pause: session.pause,
            end: session.end,
            delivery: session.delivery,
            pre_origin_inputs: session.pre_origin_inputs,
        },
        config,
        control,
        host,
        None::<(
            &mut crate::practice_playback::PracticePlayback,
            &mut crate::practice_playback::NoopPracticeRecording,
        )>,
    )
}

pub fn run_cohort_audio_with_policies_and_results_and_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
    G: GroupCompetitionPort,
>(
    device: &mut D,
    mut session: AudioCohortSession<'_, S, G>,
    config: AudioGameplayConfig,
    control: &mut C,
    host: &mut H,
    policies: &[(PlayerId, &crate::play_policy::ResolvedPlayPolicy)],
) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
    crate::native_policy_admission::validate_audio_config(&config)?;
    let section_start = config.section_start;
    let config = config.gameplay;
    let epoch = session.discipline.authority().epoch();
    validate_audio_cohort_start(&mut session, &config, epoch)?;
    crate::native_gameplay_host::validate_play_policy_members(policies)?;
    crate::native_policy_admission::validate_config(&config)?;
    if !(2..=64).contains(&session.states.len())
        || !session
            .group
            .player_ids()
            .eq(session.states.iter().map(|state| state.player))
        || !session
            .states
            .iter()
            .map(|state| state.player)
            .eq(policies.iter().map(|(id, _)| *id))
    {
        return Err("selected audio cohort requires exact original-ID roster".into());
    }
    let custom = policies
        .iter()
        .any(|(_, policy)| policy.gauge() != &GaugeProfile::default());
    if custom
        && session
            .network
            .as_ref()
            .is_some_and(|network| !network.policy_agnostic())
    {
        return Err("nondefault cohort networking requires policy-aware identities".into());
    }
    for (state, (_, policy)) in session.states.iter().zip(policies) {
        if state.score != ScoreSummary::default() || state.last_song != config.song_origin {
            return Err("selected audio cohort requires pristine member scores".into());
        }
        let judge = session
            .group
            .member_judge(state.player)
            .ok_or("selected audio member lacks judge")?;
        let header = state
            .competition
            .as_ref()
            .filter(|port| !port.policy_agnostic())
            .map(|port| {
                port.expected_policy_header()
                    .ok_or("selected audio competition lacks identity")
            })
            .transpose()?;
        crate::native_policy_admission::validate_selected_in_domain(
            judge,
            &state.gauge,
            policy,
            state.capture.as_ref(),
            header,
            &config,
            epoch.logical_origin.domain,
            Some(section_start),
        )?;
    }
    if !custom {
        host.prepare_play_policies(policies)?;
    }
    let mut resolved = crate::native_gameplay_bridge::ResolvedGameplayHost { host, policies };
    run_cohort_audio_with_results_and_ports(
        device,
        session,
        AudioGameplayConfig {
            gameplay: config,
            section_start,
        },
        control,
        &mut resolved,
    )
}

pub fn run_cohort_audio_practice_with_policies_and_results_and_ports<
    D: crate::gameplay_presentation::GameplayDevice,
    C: NativePumpControl,
    H: NativeGameplayHost,
    S: SoloCompetitionPort,
    G: GroupCompetitionPort,
    R: crate::practice_playback::PracticeRecordingPort,
>(
    device: &mut D,
    mut session: AudioCohortSession<'_, S, G>,
    config: AudioGameplayConfig,
    control: &mut C,
    host: &mut H,
    policies: &[(PlayerId, &crate::play_policy::ResolvedPlayPolicy)],
    practice: &mut crate::practice_playback::PracticePlayback,
    recording: &mut R,
) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
    crate::native_policy_admission::validate_audio_config(&config)?;
    let section_start = config.section_start;
    let config = config.gameplay;
    let epoch = session.discipline.authority().epoch();
    validate_audio_cohort_start(&mut session, &config, epoch)?;
    crate::native_gameplay_host::validate_play_policy_members(policies)?;
    crate::native_policy_admission::validate_config(&config)?;
    if !(2..=64).contains(&session.states.len())
        || !session
            .group
            .player_ids()
            .eq(session.states.iter().map(|state| state.player))
        || !session
            .states
            .iter()
            .map(|state| state.player)
            .eq(policies.iter().map(|(id, _)| *id))
    {
        return Err("selected audio cohort requires exact original-ID roster".into());
    }
    let custom = policies
        .iter()
        .any(|(_, policy)| policy.gauge() != &GaugeProfile::default());
    if custom
        && session
            .network
            .as_ref()
            .is_some_and(|network| !network.policy_agnostic())
    {
        return Err("nondefault cohort networking requires policy-aware identities".into());
    }
    for (state, (_, policy)) in session.states.iter().zip(policies) {
        if state.score != ScoreSummary::default() || state.last_song != config.song_origin {
            return Err("selected audio cohort requires pristine member scores".into());
        }
        let judge = session
            .group
            .member_judge(state.player)
            .ok_or("selected audio member lacks judge")?;
        let header = state
            .competition
            .as_ref()
            .filter(|port| !port.policy_agnostic())
            .map(|port| {
                port.expected_policy_header()
                    .ok_or("selected audio competition lacks identity")
            })
            .transpose()?;
        crate::native_policy_admission::validate_selected_in_domain(
            judge,
            &state.gauge,
            policy,
            state.capture.as_ref(),
            header,
            &config,
            epoch.logical_origin.domain,
            Some(section_start),
        )?;
    }
    if !custom {
        host.prepare_play_policies(policies)?;
    }
    let mut resolved = crate::native_gameplay_bridge::ResolvedGameplayHost { host, policies };
    run_cohort_audio_practice_with_results_and_ports(
        device,
        session,
        AudioGameplayConfig {
            gameplay: config,
            section_start,
        },
        control,
        &mut resolved,
        practice,
        recording,
    )
}

#[cfg(test)]
mod retained_practice_fixtures {
    use super::*;
    use crate::{
        audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
        bgm::BgmConfig,
        gameplay_competition::{NoopGroupCompetition, NoopSoloCompetition},
        gameplay_presentation::GameplayDevice,
        local_runtime::MemberConfig,
        native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
        native_gameplay::{retain_input, InputBatch},
        native_gameplay_host::NoopGameplayHost,
        play_policy::ResolvedPlayPolicy,
        practice_control::{PracticeCapability, PracticeReply},
        practice_playback::{PracticeMember, PracticePlayback, PracticeRecordingPort},
        session_launch::SessionLaunch,
    };
    use beatkernel::{
        audio::*,
        input::{
            Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
            EventMeta, GameControlId, PhysicalControlId,
        },
        replay::{codec::ReplayCodecLimits, ReplayOperation},
        runtime::{RuntimeProcessingClock, SoundBinding},
        time::{presentation::PresentationEstimator, ClockPair, Duration, ExtrapolationPolicy},
        transport::{Rate, Transport},
    };
    use beatkernel_platform::audio::presentation::validation::{
        NativePresentationValidator, OriginalNativePresentationEvidence,
    };
    fn at(domain: u32, ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(domain),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn limits() -> ReplayCodecLimits {
        ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
    }
    struct Device {
        mixer: Mixer,
        basis: OutputFrameBasis,
        rendered: Option<RenderReport>,
        step: u64,
        sources: Vec<DeviceId>,
        loop_frames: u64,
        pcm: Vec<f32>,
        acquired: Vec<PhysicalInputEvent>,
        failed_gauge: bool,
        unknown_stops_before_first_cut: u64,
    }
    impl GameplayDevice for Device {
        type Presentation = PresentationEstimator;
        fn observe(&mut self, _: &mut Self::Presentation) -> NativeGameplayResult<()> {
            Err("legacy timing must not run for retained cohort".into())
        }
        fn observe_audio(
            &mut self,
            presentation: &mut NativeAudioPresentation,
        ) -> NativeGameplayResult<()> {
            self.step += 1;
            let mut pcm = [0.0; 10];
            self.rendered = Some(self.mixer.render(&mut pcm)?);
            if self.mixer.playback_frame_cursor() < self.loop_frames {
                self.unknown_stops_before_first_cut = self.rendered.unwrap().counters.unknown_stops;
            }
            self.pcm.extend(pcm);
            let point = self.step as i64 * 10_000_000;
            presentation.admit(NativeAudioSnapshot {
                epoch: 0,
                basis: self.basis,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                    source: at(2, point),
                    target: at(1, point),
                }),
            })?;
            Ok(())
        }
        fn audio_pause_observation(
            &mut self,
            presentation: &NativeAudioPresentation,
            _: ClockPoint,
        ) -> NativeGameplayResult<crate::live_pause::LivePauseObservation> {
            let record = presentation
                .latest_record()
                .ok_or("cohort fixture lacks original native observation")?;
            Ok(crate::live_pause::LivePauseObservation::Point(
                record.pair(),
            ))
        }
        fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
            Ok(self.rendered)
        }
        fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
            Ok(at(1, self.step as i64 * 10_000_000))
        }
        fn acquire(
            &mut self,
            events: &mut VecDeque<PhysicalInputEvent>,
        ) -> NativeGameplayResult<InputBatch> {
            // Original HOST acquisition contains repeated hits and inputs exactly
            // on loop cuts. Different members retain their own source sequences.
            for frame in (self.step - 1) * 10 + 1..=self.step * 10 {
                if self.loop_frames == 31 && frame == 31 {
                    for (i, source) in self.sources.iter().copied().enumerate().rev() {
                        let event = PhysicalInputEvent::Button(ButtonEvent {
                            meta: EventMeta::new(source, at(1, 30_500_000), 61),
                            control: PhysicalControlId::keyboard(4 + i as u16),
                            state: ButtonState::Up,
                        });
                        self.acquired.push(event.clone());
                        retain_input(events, event)?;
                    }
                }
                let state = match frame % self.loop_frames {
                    0 if self.loop_frames == 31 && frame == 31 => ButtonState::Down,
                    10 => ButtonState::Down,
                    20 | 0 => ButtonState::Up,
                    _ => continue,
                };
                for (i, source) in self.sources.iter().copied().enumerate().rev() {
                    let event = PhysicalInputEvent::Button(ButtonEvent {
                        meta: EventMeta::new(source, at(1, frame as i64 * 1_000_000), frame * 2),
                        control: PhysicalControlId::keyboard(4 + i as u16),
                        state,
                    });
                    self.acquired.push(event.clone());
                    retain_input(events, event)?;
                }
            }
            Ok(InputBatch {
                completed_through: Some(self.host_now()?),
                backlog: false,
                closed: self.step >= if self.failed_gauge { 25 } else { 12 },
            })
        }
        fn observe_end(
            &mut self,
            _: &mut NativeEnd,
            _: &Self::Presentation,
            _: Option<RenderReport>,
        ) -> NativeGameplayResult<Option<crate::native_end::EndBoundary>> {
            Err("legacy endpoint must not run".into())
        }
        fn seed_resume(
            &mut self,
            _: &mut Self::Presentation,
            _: ClockPair,
        ) -> NativeGameplayResult<()> {
            Err("retained practice must not reseed its authority".into())
        }
        fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
            Ok(at(2, self.mixer.playback_frame_cursor() as i64 * 1_000_000))
        }
    }
    #[derive(Default)]
    struct Host {
        reports: Vec<Vec<PlayerReport>>,
        generations: Vec<u64>,
        refuse_visual: bool,
        identities: Vec<u64>,
        stops: Vec<(PlayerId, AudioCommand)>,
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
        fn prepare_play_policies(
            &mut self,
            policies: &[(PlayerId, &ResolvedPlayPolicy)],
        ) -> NativeGameplayResult<()> {
            NoopGameplayHost.prepare_play_policies(policies)
        }
        fn publish_report(
            &mut self,
            _: &beatkernel::runtime::RuntimeReport,
        ) -> NativeGameplayResult<()> {
            Ok(())
        }
        fn publish_local_reports(&mut self, reports: &[PlayerReport]) -> NativeGameplayResult<()> {
            self.reports.push(reports.to_vec());
            Ok(())
        }
        fn diagnostic(&mut self, diagnostic: NativeGameplayDiagnostic<'_>) {
            if let NativeGameplayDiagnostic::LocalReport { player, report } = diagnostic {
                self.stops
                    .extend(report.audio_commands.iter().filter_map(|command| {
                        matches!(command, AudioCommand::Stop { .. }).then_some((player, *command))
                    }));
            }
        }
        fn advertise_practice(
            &mut self,
            capability: Option<PracticeCapability>,
        ) -> NativeGameplayResult<()> {
            if let Some(capability) = capability {
                self.generations.push(capability.generation);
            }
            Ok(())
        }
        fn commit_practice_reply(&mut self, _: &PracticeReply) -> NativeGameplayResult<()> {
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
            _: &mut crate::player::PreparedPracticePresentation,
            generation: u64,
        ) {
            self.identities.push(generation);
        }
        fn commit_practice_presentation(
            &mut self,
            _: crate::player::PreparedPracticePresentation,
            _: u64,
        ) -> NativeGameplayResult<()> {
            if self.refuse_visual {
                return Err("literal cohort visual refusal".into());
            }
            Ok(())
        }
    }
    #[derive(Default)]
    struct Recordings(Vec<(PlayerId, PathBuf, LiveReplayCapture)>, bool);
    impl PracticeRecordingPort for Recordings {
        fn archive(
            &mut self,
            player: PlayerId,
            path: &Path,
            capture: LiveReplayCapture,
        ) -> NativeGameplayResult<()> {
            self.0.push((player, path.to_owned(), capture));
            if self.1 && path.to_string_lossy().contains(".retry") {
                return Err("literal final archive publication refusal".into());
            }
            Ok(())
        }
    }
    struct Control;
    impl NativePumpControl for Control {
        type Moment = u64;
        fn now(&mut self) -> NativeGameplayResult<u64> {
            Err("unbounded practice has no wall deadline".into())
        }
        fn checked_add(now: u64, duration: WallDuration) -> Option<u64> {
            now.checked_add(duration.as_nanos().try_into().ok()?)
        }
        fn wait(&mut self, _: WallDuration) -> NativeGameplayResult<()> {
            Ok(())
        }
    }
    #[test]
    fn actual_retained_cohort_pump_shares_boundary_and_preserves_independent_original_captures() {
        run_retained_cohort(false);
    }
    #[test]
    fn actual_retained_cohort_failed_gauge_keeps_executed_stop_evidence_across_attempts() {
        run_retained_cohort(true);
    }
    fn run_retained_cohort(failed_gauge: bool) {
        let regions: &[(i64, u64)] = if failed_gauge {
            &[(100_000_000, 100)]
        } else {
            &[(30_000_000, 30), (30_000_001, 31)]
        };
        let visual_refusals: &[bool] = if failed_gauge {
            &[false]
        } else {
            &[false, true]
        };
        for &(section_end_ns, loop_frames) in regions {
            for &refuse_visual in visual_refusals {
                for count in 2..=4 {
                    let source = beatkernel_bms::parse(
                        if failed_gauge {
                            "#BPM 6000\n#WAV00 mine.wav\n#WAV01 key.wav\n#00011:00010001\n#000D1:000000ZZ00000000\n"
                        } else {
                            "#BPM 6000\n#WAV01 key.wav\n#00011:00010000\n"
                        },
                        Default::default(),
                    )
                    .unwrap();
                    let unused_voice_object =
                        failed_gauge.then(|| source.compile().unwrap().chart.objects()[1].id);
                    let policy = ResolvedPlayPolicy::builtin(0, 0, 0).unwrap();
                    assert_eq!(policy.judge().windows()[0].early, Duration::ZERO);
                    assert_eq!(policy.judge().windows()[0].late, Duration::ZERO);
                    assert_eq!(policy.judge().input_offset(), Duration::ZERO);
                    let format = AudioFormat::new(1000, 1).unwrap();
                    let pcm_limits = PcmLimits::new(64, 256, if failed_gauge { 2 } else { 1 }).unwrap();
                    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
                    bank.insert(
                        SampleId(1),
                        PcmSample::new(format, vec![0.25; 4], pcm_limits).unwrap(),
                    )
                    .unwrap();
                    if failed_gauge {
                        bank.insert(
                            SampleId(0),
                            PcmSample::new(format, vec![0.0; 4], pcm_limits).unwrap(),
                        )
                        .unwrap();
                    }
                    let program = PreparedPracticeProgram::new(
                        &bank,
                        vec![PracticeCue {
                            voice: VoiceId(900),
                            sample: SampleId(1),
                            at: Timestamp::ZERO,
                            gain: 1.0,
                        }],
                        PracticeLimits::new(4, 1, 64, 8, 64).unwrap(),
                    )
                    .unwrap();
                    let (controller, endpoint) = practice_queue(&program).unwrap();
                    let (producer, consumer) = command_queue(128).unwrap();
                    let mut mixer = Mixer::new(
                        MixerConfig::new(
                            format,
                            ClockDomainId(2),
                            Timestamp::ZERO,
                            AudioLimits::new(128, 16, 16, 16, 16).unwrap(),
                        ),
                        bank,
                        consumer,
                    )
                    .unwrap();
                    let region = PracticeRegion::new(
                        Timestamp::ZERO,
                        Timestamp::from_nanos(section_end_ns),
                        true,
                    )
                    .unwrap();
                    mixer.install_practice(program, endpoint, region).unwrap();
                    let basis = mixer.output_frame_basis();
                    let sources: Vec<_> = (0..count).map(|i| DeviceId(101 + i as u64)).collect();
                    let mut members = Vec::new();
                    let mut states = Vec::new();
                    let mut practice_members = Vec::new();
                    let mut member_headers = Vec::new();
                    for i in 0..count {
                        let player = PlayerId(2 + i as u32);
                        let compiled = source.compile().unwrap();
                        assert_eq!(
                            compiled.chart.objects().len(),
                            if failed_gauge { 2 } else { 1 }
                        );
                        assert_eq!(
                            compiled.chart.objects()[0].time.start,
                            Timestamp::from_nanos(10_000_000)
                        );
                        let sounds = compiled
                            .chart
                            .objects()
                            .iter()
                            .map(|object| SoundBinding {
                                object: object.id,
                                stage: beatkernel::judge::JudgeStage::Instant,
                                sample: SampleId(1),
                                voice: VoiceId(100 * (i as u64 + 1) + object.id.0),
                                gain: 1.0,
                            })
                            .collect();
                        let judge = crate::mine_plan::prepare_judge(
                            &source,
                            compiled.chart,
                            policy.judge().clone(),
                            beatkernel_bms::BmsInputMode::ButtonOnly,
                            beatkernel_bms::ParseOptions::default().max_objects,
                        )
                        .unwrap();
                        let capture = LiveReplayCapture::new_with_policy(
                            &judge,
                            ClockDomainId(3),
                            limits(),
                            Timestamp::ZERO,
                            0,
                            Some(region.end),
                            beatkernel_bms::BmsInputMode::ButtonOnly,
                            None,
                            &policy,
                        )
                        .unwrap();
                        member_headers.push((player, capture.header().clone()));
                        members.push(MemberConfig {
                            player,
                            device: Some(sources[i]),
                            bindings: BindingMap::from_bindings([Binding {
                                device: DeviceSelector::Exact(sources[i]),
                                physical: PhysicalControlId::keyboard(4 + i as u16),
                                game_control: GameControlId(0x11),
                            }])
                            .unwrap(),
                            judge,
                            sounds,
                        });
                        states.push(GameplayPlayerState {
                            player,
                            capture: Some(capture),
                            competition: Some(NoopSoloCompetition),
                            completion: None,
                            score: ScoreSummary::default(),
                            gauge: BmsGauge::default(),
                            last_song: Timestamp::ZERO,
                        });
                        practice_members.push(PracticeMember {
                            player,
                            policy: ResolvedPlayPolicy::builtin(0, 0, 0).unwrap(),
                            launch: SessionLaunch::new(vec![
                                "--chart".into(),
                                "cohort.bms".into(),
                                "--record-replay".into(),
                                "cohort.bkr".into(),
                            ])
                            .unwrap()
                            .for_recording_path(
                                replay_path(Path::new("cohort.bkr"), player).unwrap(),
                                replay_path(Path::new("cohort.bkr"), player).unwrap(),
                            )
                            .unwrap(),
                            chart_seed: 0,
                            capture_limits: Some(limits()),
                        });
                    }
                    let mut group = RuntimeGroup::new(
                        ClockDomainId(3),
                        ClockDomainId(2),
                        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
                        producer,
                        members,
                        8,
                        &[],
                    )
                    .unwrap();
                    group.set_song_end(region.end).unwrap();
                    group.set_processing_clock(RuntimeProcessingClock::Disabled);
                    let mut presentation = NativeAudioPresentation::new(
                        AudioAuthority::new(
                            AudioAuthorityConfig {
                                history_capacity: 32,
                                max_observation_age: Duration::from_nanos(1_000_000_000),
                                input_extrapolation: ExtrapolationPolicy::Forbid,
                                max_input_ahead: Duration::ZERO,
                            },
                            AudioAuthorityEpoch {
                                id: 0,
                                stream_origin: at(2, 0),
                                logical_origin: at(3, 0),
                                host_domain: ClockDomainId(1),
                            },
                        )
                        .unwrap(),
                        NativePresentationValidator::new(0, at(2, 0), ClockDomainId(1)),
                    )
                    .unwrap();
                    presentation
                        .admit(NativeAudioSnapshot {
                            epoch: 0,
                            basis,
                            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                                source: at(2, 0),
                                target: at(1, 0),
                            }),
                        })
                        .unwrap();
                    let mut device = Device {
                        mixer,
                        basis,
                        rendered: None,
                        step: 0,
                        sources: sources.clone(),
                        loop_frames,
                        pcm: vec![],
                        acquired: vec![],
                        failed_gauge,
                        unknown_stops_before_first_cut: 0,
                    };
                    let mut playback = PracticePlayback::new(
                        controller,
                        source,
                        practice_members,
                        region,
                        Timestamp::from_nanos(100_000_000),
                        basis,
                        2,
                    )
                    .unwrap();
                    let mut merger =
                        InputMerger::new(ClockDomainId(1), at(1, 0), sources, 128).unwrap();
                    let mut bgm = BgmFeeder::new(
                        vec![],
                        BgmConfig {
                            output_origin: at(2, 0),
                            sample_rate: 1000,
                            preroll: Duration::ZERO,
                            lookahead: Duration::from_nanos(100_000_000),
                            max_pending: 1,
                        },
                    )
                    .unwrap();
                    let mut pause = NativePause::new(at(2, 0), ClockDomainId(1), 1000).unwrap();
                    let mut end = None;
                    let mut delivery = InputDeliveryTelemetry::new(128, ClockDomainId(1)).unwrap();
                    let mut pre = 0;
                    let mut host = Host {
                        refuse_visual,
                        ..Host::default()
                    };
                    let mut recordings = Recordings::default();
                    recordings.1 = refuse_visual;
                    let policies: Vec<_> =
                        states.iter().map(|state| (state.player, &policy)).collect();
                    let result = run_cohort_audio_practice_with_policies_and_results_and_ports(
                        &mut device,
                        AudioCohortSession {
                            group: &mut group,
                            network: None::<&mut NoopGroupCompetition>,
                            states: &mut states,
                            merger: &mut merger,
                            bgm: &mut bgm,
                            discipline: &mut presentation,
                            pause: &mut pause,
                            end: &mut end,
                            delivery: &mut delivery,
                            pre_origin_inputs: &mut pre,
                        },
                        AudioGameplayConfig {
                            gameplay: NativeGameplayConfig {
                                origin: at(1, 0),
                                stream_origin: at(2, 0),
                                playback_origin: at(2, 0),
                                song_origin: Timestamp::ZERO,
                                sample_rate: 1000,
                                end_song: Some(region.end),
                                advance_lag: Duration::ZERO,
                                seconds: None,
                                pause_supported: false,
                                logical_schedule: true,
                            },
                            section_start: Timestamp::ZERO,
                        },
                        &mut Control,
                        &mut host,
                        &policies,
                        &mut playback,
                        &mut recordings,
                    );
                    if section_end_ns == 30_000_001 {
                        let prefix = presentation.authority().acquired_prefix().unwrap();
                        assert!(prefix.timestamp >= Timestamp::from_nanos(40_000_000));
                        assert!(merger.peek_ready(prefix).unwrap().is_none_or(|event| {
                            event.meta().timestamp >= Timestamp::from_nanos(31_000_000)
                        }));
                        for player in (2..2 + count as u32).map(PlayerId) {
                            let source = DeviceId(101 + u64::from(player.0 - 2));
                            for (ns, sequence, state) in [
                                (30_500_000, 61, ButtonState::Up),
                                (31_000_000, 62, ButtonState::Down),
                            ] {
                                let original = device
                                    .acquired
                                    .iter()
                                    .find(|event| {
                                        event.meta().source == source
                                            && event.meta().timestamp == Timestamp::from_nanos(ns)
                                    })
                                    .unwrap();
                                assert_eq!(original.meta().clock_domain, ClockDomainId(1));
                                assert_eq!(original.meta().sequence, sequence);
                                assert_eq!(original.meta().original_clock_point, None);
                                assert!(matches!(original, PhysicalInputEvent::Button(button)
                                    if button.state == state));
                            }
                            // The single-member report comes from draining the
                            // original 30.5 ms event; boundary Advance fans out
                            // to every member separately at the actual 31 ms cut.
                            let gap = host
                                .reports
                                .iter()
                                .find(|reports| {
                                    reports.len() == 1
                                        && reports[0].player == player
                                        && reports[0].report.song_end_reached
                                        && reports[0].report.audio_at == at(2, 40_000_000)
                                })
                                .unwrap();
                            let report = &gap[0].report;
                            assert_eq!(report.song_time, Timestamp::from_nanos(30_000_001));
                            assert!(report.input.is_none());
                            assert!(report.bound_inputs.is_empty());
                            assert!(report.judge_events.is_empty());
                            assert!(report.hazard_events.is_empty());
                            assert!(report.audio_commands.is_empty());
                            assert!(report.audio_failures.is_empty());
                            assert!(report.judge_error.is_none());
                            let (_, _, old) = recordings
                                .0
                                .iter()
                                .find(|(id, _, _)| *id == player)
                                .unwrap();
                            assert_eq!(old.header(), &member_headers[(player.0 - 2) as usize].1);
                            assert_eq!(
                                old.records()
                                    .iter()
                                    .filter(|record| {
                                        record.song_time == Timestamp::from_nanos(30_000_001)
                                            && matches!(record.operation, ReplayOperation::Advance)
                                    })
                                    .count(),
                                2
                            );
                            assert!(old.records().iter().all(|record| {
                                record.song_time <= Timestamp::from_nanos(30_000_001)
                                    && !matches!(&record.operation, ReplayOperation::Input(input)
                                        if input.physical.meta().original_clock_point.is_some_and(|point|
                                            point.timestamp >= Timestamp::from_nanos(30_500_000)))
                            }));
                        }
                    }
                    if refuse_visual {
                        assert_eq!(
                            result.as_ref().unwrap_err().to_string(),
                            "literal cohort visual refusal"
                        );
                        assert_eq!(playback.generation(), 2);
                        assert_eq!(group.audio_scope(), CommandScope(2));
                        assert_eq!(host.identities, vec![2]);
                        for state in &states {
                            assert_eq!(state.score, ScoreSummary::default());
                            assert!(state
                                .capture
                                .as_ref()
                                .is_some_and(|capture| capture.records().is_empty()));
                            let member = playback
                                .members()
                                .iter()
                                .find(|member| member.player == state.player)
                                .unwrap();
                            let path = member
                                .launch
                                .args()
                                .chunks_exact(2)
                                .find(|pair| pair[0] == "--record-replay")
                                .unwrap();
                            assert_eq!(path[1], format!("cohort.p{}.retry1.bkr", state.player.0));
                        }
                        let save_paths: Vec<_> = playback
                            .members()
                            .iter()
                            .map(|member| {
                                let path = member
                                    .launch
                                    .args()
                                    .chunks_exact(2)
                                    .find(|pair| pair[0] == "--record-replay")
                                    .unwrap();
                                (member.player, Some(PathBuf::from(&path[1])))
                            })
                            .collect();
                        let lookup = save_paths.clone();
                        let final_states = states
                            .drain(..)
                            .map(|state| PlayerState {
                                player: state.player,
                                capture: state.capture,
                                competition: None,
                                completion: state.completion,
                                score: state.score,
                                gauge: state.gauge,
                                last_song: state.last_song,
                            })
                            .collect();
                        let finished =
                            crate::native_cohort_setup::finish_cohort_with_results_and_network(
                                result,
                                final_states,
                                None,
                                save_paths,
                                vec![],
                                None,
                                |capture, path, failed| {
                                    assert!(failed);
                                    if let Some(capture) = capture {
                                        let path = path.unwrap();
                                        let player = lookup
                                            .iter()
                                            .find(|(_, candidate)| {
                                                candidate.as_deref() == Some(path)
                                            })
                                            .unwrap()
                                            .0;
                                        recordings.archive(player, path, capture)?;
                                    }
                                    Ok(())
                                },
                                |_, _| Ok(()),
                            );
                        assert_eq!(
                            finished.unwrap_err().to_string(),
                            "literal cohort visual refusal"
                        );
                        for player in (2..2 + count as u32).map(PlayerId) {
                            let takes: Vec<_> = recordings
                                .0
                                .iter()
                                .filter(|(id, _, _)| *id == player)
                                .collect();
                            assert_eq!(takes.len(), 2);
                            assert_eq!(
                                takes[0].1,
                                PathBuf::from(format!("cohort.p{}.bkr", player.0))
                            );
                            assert_eq!(
                                takes[1].1,
                                PathBuf::from(format!("cohort.p{}.retry1.bkr", player.0))
                            );
                            assert!(takes[1].2.records().is_empty());
                        }
                        continue;
                    }
                    assert!(result.unwrap().is_none());
                    assert!(playback.generation() >= 3);
                    assert_eq!(presentation.authority().epoch().id, 0);
                    assert_eq!(device.basis, basis);
                    assert_eq!(group.audio_scope(), CommandScope(playback.generation()));
                    if failed_gauge {
                        // The unused 30 ms head has never played when the held
                        // 15 ms mine fences each member and admits its Stop.
                        // Those genuine Stops execute before the first 100 ms
                        // cut, and remain counted on the same retained Mixer.
                        assert!(device.unknown_stops_before_first_cut >= count as u64);
                        let final_report = device.rendered.unwrap();
                        assert!(
                            final_report.counters.unknown_stops
                                > device.unknown_stops_before_first_cut
                        );
                        assert!(host.identities.len() >= 2);
                        let unused = unused_voice_object.unwrap();
                        for state in &states {
                            assert_eq!(
                                state.gauge.snapshot().failure,
                                Some(crate::gauge::GaugeFailure::InstantDeath)
                            );
                            assert_eq!(state.score.hits, 1);
                            assert!(state.capture.is_some());
                            let voice = VoiceId(100 * u64::from(state.player.0 - 1) + unused.0);
                            assert!(host.stops.iter().any(|(player, command)| {
                                *player == state.player
                                    && matches!(command,
                                    AudioCommand::Stop { voice: stopped, .. } if *stopped == voice)
                            }));
                            let takes: Vec<_> = recordings
                                .0
                                .iter()
                                .filter(|(player, _, _)| *player == state.player)
                                .collect();
                            assert!(takes.len() >= 2);
                            assert_eq!(
                                takes[0].1,
                                PathBuf::from(format!("cohort.p{}.bkr", state.player.0))
                            );
                            assert_eq!(
                                takes[1].1,
                                PathBuf::from(format!("cohort.p{}.retry1.bkr", state.player.0))
                            );
                            assert!(takes[1].2.records().iter().any(|record| {
                                matches!(&record.operation, ReplayOperation::Input(input)
                                    if input.physical.meta().original_clock_point
                                        == Some(at(1, 110_000_000)))
                            }));
                        }
                        for frame in [0, 100, 200] {
                            assert_eq!(device.pcm[frame], 0.25);
                        }
                        continue;
                    }
                    for state in &states {
                        assert_eq!(state.score.hits, 1);
                        assert!(
                            state.capture.is_some(),
                            "outer native finisher retains final capture ownership"
                        );
                    }
                    for player in (2..2 + count as u32).map(PlayerId) {
                        let takes: Vec<_> = recordings
                            .0
                            .iter()
                            .filter(|(id, _, _)| *id == player)
                            .collect();
                        assert!(takes.len() >= 2);
                        for (ordinal, (_, path, capture)) in takes.into_iter().enumerate() {
                            let expected = if ordinal == 0 {
                                PathBuf::from(format!("cohort.p{}.bkr", player.0))
                            } else {
                                PathBuf::from(format!("cohort.p{}.retry{ordinal}.bkr", player.0))
                            };
                            assert_eq!(path, &expected);
                            assert_eq!(capture.header().normalized_clock, ClockDomainId(3));
                            assert_eq!(
                                capture.header(),
                                &member_headers[(player.0 - 2) as usize].1
                            );
                            assert!(capture.records().iter().any(|record| matches!(&record.operation, ReplayOperation::Input(input) if input.physical.meta().source == DeviceId(101 + u64::from(player.0 - 2)))));
                        }
                        if section_end_ns == 30_000_001 {
                            let (_, _, fresh) = recordings
                                .0
                                .iter()
                                .filter(|(id, _, _)| *id == player)
                                .nth(1)
                                .unwrap();
                            let equal_cut = fresh.records().iter().find(|record| {
                                matches!(&record.operation, ReplayOperation::Input(input)
                                    if input.physical.meta().original_clock_point == Some(at(1, 31_000_000)))
                            }).unwrap();
                            assert_eq!(equal_cut.song_time, Timestamp::ZERO);
                            let ReplayOperation::Input(input) = &equal_cut.operation else {
                                unreachable!();
                            };
                            assert_eq!(input.physical.meta().clock_domain, ClockDomainId(3));
                            assert_eq!(
                                input.physical.meta().timestamp,
                                Timestamp::from_nanos(31_000_000)
                            );
                            assert_eq!(
                                input.physical.meta().source,
                                DeviceId(101 + u64::from(player.0 - 2))
                            );
                            assert_eq!(input.physical.meta().sequence, 62);
                            assert!(matches!(&input.physical, PhysicalInputEvent::Button(button)
                                if button.state == ButtonState::Down));
                            let fresh_report = host
                                .reports
                                .iter()
                                .flatten()
                                .find(|tagged| {
                                    tagged.player == player
                                        && tagged.report.input.as_ref().is_some_and(|input| {
                                            input.meta().original_clock_point
                                                == Some(at(1, 31_000_000))
                                        })
                                })
                                .unwrap();
                            assert_eq!(fresh_report.report.song_time, Timestamp::ZERO);
                            assert!(!fresh_report.report.song_end_reached);
                            assert_eq!(fresh_report.report.bound_inputs.len(), 1);
                            // The fresh Down is bound and recorded at song 0,
                            // before the original 10 ms head's zero-width window.
                            assert!(fresh_report.report.judge_events.is_empty());
                            assert!(fresh_report.report.audio_commands.is_empty());
                        }
                    }
                    for lap in 0..4 {
                        assert_eq!(device.pcm[(lap * loop_frames) as usize], 0.25);
                    }
                    // Equal-cut originals belong to the newly anchored attempt, while
                    // acquired provenance remains HOST1 through capture normalization.
                    assert!(host.reports.iter().flatten().any(|tagged| {
                        tagged.report.input.as_ref().is_some_and(|input| {
                            input.meta().original_clock_point
                                == Some(at(1, loop_frames as i64 * 1_000_000))
                        }) && tagged.report.song_time == Timestamp::ZERO
                    }));
                }
            }
        }
    }
}
