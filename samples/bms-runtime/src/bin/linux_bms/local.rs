//! Actual Linux local cohort: independent input/judging, one native audio owner.
use super::native::{ExplicitDomains, HOST, OUTPUT, observe, output_origin, seed};
use super::*;
use beatkernel::{
    audio::{AudioCommand, Mixer, MixerConfig, PcmLimits, command_queue},
    input::{Binding, BindingMap, DeviceId, DeviceSelector, GameControlId, PhysicalControlId},
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    replay::codec::ReplayCodecLimits,
    time::Duration,
    transport::{Rate, Transport},
};
use beatkernel_bms_runtime::{
    ChannelPolicy,
    competition::ScoreSummary,
    competition_live::{CompetitionOptions, LiveCompetition},
    completion::SongCompletion,
    load_prepared_with_seed,
    local_input::InputMerger,
    local_players::{MAX_LOCAL_PLAYERS, PlayerId},
    local_runtime::{InputResult, MemberConfig, PlayerReport, RuntimeGroup, VoiceAllocator},
    native_end::NativeEnd,
    playback_pause::{NativePause, PauseKeyboard, PausePhase},
    player::{self, PauseState},
    replay_capture::LiveReplayCapture,
};
use beatkernel_platform::{
    audio::{
        DeviceFormat, SampleEncoding,
        presentation::discipline::{DisciplineConfig, DisciplineUpdate, PresentationDiscipline},
    },
    linux::{AlsaRequest, AlsaStream, EvdevDevice, EvdevItem, MonotonicClock},
};
use std::{
    path::Path,
    time::{Duration as WallDuration, Instant},
};

struct PlayerState {
    player: PlayerId,
    capture: Option<LiveReplayCapture>,
    competition: Option<LiveCompetition>,
    completion: Option<SongCompletion>,
    score: ScoreSummary,
    last_song: Timestamp,
}

/// A candidate watermark is insufficient: every member requires the same
/// actually committed acquisition frontier and acknowledged native endpoint.
fn finite_cohort_done(
    end: Option<i64>,
    presented: Option<ClockPoint>,
    committed: Option<ClockPoint>,
    states: &[PlayerState],
    backlog: bool,
    resuming: bool,
) -> bool {
    committed.is_some_and(|frontier| {
        states.iter().all(|state| {
            finite_session_done(end, presented, frontier, state.last_song, backlog, resuming)
        })
    })
}

fn admit_mode(count: usize, network: bool) -> Result<()> {
    if !(2..=MAX_LOCAL_PLAYERS).contains(&count) {
        return Err("Linux local play requires 2..64 explicit inputs".into());
    }
    if network {
        return Err("network competition currently supports one local participant only".into());
    }
    Ok(())
}

/// Uses the base stem, preserves its directory/OS encoding, and adds a player
/// suffix. Actual save still uses the parent's create-new/no-overwrite boundary.
fn replay_path(base: &Path, player: PlayerId) -> Result<PathBuf> {
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

/// Preserve every actual completed report, including reports from a partial
/// group error, before propagating a capture/competition error to cleanup.
fn observe_reports(reports: &[PlayerReport], states: &mut [PlayerState]) -> Result<()> {
    let mut failures = Vec::new();
    for tagged in reports {
        let Some(state) = states
            .iter_mut()
            .find(|state| state.player == tagged.player)
        else {
            failures.push(format!("report for unknown player {:?}", tagged.player));
            continue;
        };
        state.last_song = tagged.report.song_time;
        if let Some(capture) = state.capture.as_mut() {
            if let Err(error) = capture.record_report(&tagged.report) {
                failures.push(format!("player{} capture: {error}", state.player.0));
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
        for event in &tagged.report.judge_events {
            println!("player{} judge={event:?}", tagged.player.0);
        }
        if tagged.report.judge_error.is_some() || !tagged.report.audio_failures.is_empty() {
            eprintln!(
                "player{} committed partial report judge={:?}, audio={:?}",
                tagged.player.0, tagged.report.judge_error, tagged.report.audio_failures
            );
        }
    }
    if let Err(error) = beatkernel_bms_runtime::player::publish_local_reports(reports) {
        failures.push(format!("local presentation: {error}"));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; ").into())
    }
}

fn playback_schedule(pause: &NativePause, stream: &AlsaStream) -> Result<ClockPoint> {
    Ok(pause.scheduling_point(
        stream
            .last_render_report()
            .or_else(|| pause.last_render_report())
            .ok_or("local mixer playback boundary unavailable")?,
    )?)
}
fn process_event(
    event: beatkernel::input::PhysicalInputEvent,
    group: &mut RuntimeGroup,
    pause: &NativePause,
    stream: &AlsaStream,
    states: &mut [PlayerState],
) -> Result<()> {
    match group.process_input(event, &ExplicitDomains, playback_schedule(pause, stream)?) {
        Ok(InputResult::Processed(reports)) => observe_reports(&reports, states),
        Ok(InputResult::Ignored { device }) => {
            Err(format!("merged source {device:?} has no local runtime owner").into())
        }
        Err(failure) => {
            if let Err(error) = observe_reports(&failure.completed_reports, states) {
                eprintln!("partial group report observation: {error}");
            }
            Err(failure.into())
        }
    }
}
fn advance_group(
    at: ClockPoint,
    group: &mut RuntimeGroup,
    pause: &NativePause,
    stream: &AlsaStream,
    states: &mut [PlayerState],
) -> Result<()> {
    match group.advance_to(at, &ExplicitDomains, playback_schedule(pause, stream)?) {
        Ok(reports) => observe_reports(&reports, states),
        Err(failure) => {
            if let Err(error) = observe_reports(&failure.completed_reports, states) {
                eprintln!("partial group deadline observation: {error}");
            }
            Err(failure.into())
        }
    }
}
fn reconcile_resume(
    keyboard: &mut PauseKeyboard,
    at: ClockPoint,
    group: &mut RuntimeGroup,
    pause: &NativePause,
    stream: &AlsaStream,
    states: &mut [PlayerState],
) -> Result<()> {
    for event in keyboard.resume(at)? {
        process_event(event, group, pause, stream, states)?;
    }
    Ok(())
}
fn update_discipline(
    discipline: &mut PresentationDiscipline,
    now: ClockPoint,
    group: &mut RuntimeGroup,
) -> Result<()> {
    if let DisciplineUpdate::Applied {
        base_rate_ppm,
        correction_ppm,
        applied_rate_ppm,
        phase_error_ns,
        limited,
    } = discipline.update(now, group.transport_mut())?
    {
        println!(
            "shared discipline measured={base_rate_ppm:+}ppm correction={correction_ppm:+}ppm applied={applied_rate_ppm:+}ppm phase={phase_error_ns}ns limited={limited}"
        );
    }
    Ok(())
}
/// An actual pause commit may precede the configured lag watermark. Wait for
/// that first safe frontier; subsequent merger queries still detect regression.
fn lag_reaches(now: ClockPoint, boundary: ClockPoint, lag: i64) -> Result<bool> {
    if now.domain != boundary.domain || !(0..=1_000_000_000).contains(&lag) {
        return Err("local pause lag has invalid domain or extent".into());
    }
    let frontier = i128::from(now.timestamp.as_nanos())
        .checked_sub(i128::from(lag))
        .ok_or("local pause lag arithmetic overflow")?;
    Ok(frontier >= i128::from(boundary.timestamp.as_nanos()))
}

pub(super) fn run(options: Options, competition_options: CompetitionOptions) -> Result<()> {
    admit_mode(
        options.local_inputs.len(),
        competition_options.network.is_some(),
    )?;
    let playback_end = options.playback_end()?;
    let count = options.local_inputs.len();
    let song_origin = options.song_origin()?;
    // Validate both checked endpoint grids before loading assets or opening owners.
    let mut pause = NativePause::new(output_origin(), HOST, options.format.sample_rate())?;
    if let Some(end) = playback_end {
        pause = pause.with_playback_end_frame(end)?;
    }
    let mut native_end = playback_end
        .map(|end| NativeEnd::new(output_origin(), HOST, options.format.sample_rate(), end))
        .transpose()?;
    let clock = MonotonicClock::new(HOST);
    // Declared before native owners so delivery observations report after cleanup.
    let mut delivery = DeliverySession(beatkernel::telemetry::InputDeliveryTelemetry::new(
        4096, HOST,
    )?);
    let prepared = load_prepared_with_seed(
        &options.chart,
        options.format,
        PcmLimits::new(64 * 1024 * 1024, 256 * 1024 * 1024, 1295)?,
        if options.mono_stereo {
            ChannelPolicy::MonoToStereo
        } else {
            ChannelPolicy::Exact
        },
        options.chart_seed,
    )?;
    let (prepared, section) = beatkernel_bms_runtime::section_start::prepare_at(
        prepared,
        Timestamp::from_nanos(options.start_ns),
        PcmLimits::new(64 * 1024 * 1024, 256 * 1024 * 1024, 1295)?,
    )?;
    println!("prepared practice section={section:?}");
    for warning in &prepared.source.warnings {
        eprintln!("BMS warning line{}: {}", warning.line, warning.message);
    }
    for note in &prepared.source.notes {
        if !options.bindings.contains_key(&note.lane.channel()) {
            return Err(
                format!("missing --bind for BMS channel{:02X}", note.lane.channel()).into(),
            );
        }
    }
    beatkernel_bms_runtime::player::publish_local_chart(
        &prepared.source,
        &prepared.compiled.chart,
        &options.local_players,
    )?;
    let reserved: Vec<_> = prepared
        .bgm_commands
        .iter()
        .map(|command| match command {
            AudioCommand::Play { voice, .. } => Ok(*voice),
            _ => Err("prepared BGM command is not Play"),
        })
        .collect::<std::result::Result<_, _>>()?;
    let first_voice = reserved
        .iter()
        .map(|voice| voice.0)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or("local voice namespace overflow")?;
    let mut allocator = VoiceAllocator::new(first_voice);
    let mut configs = Vec::with_capacity(count);
    let mut states = Vec::with_capacity(count);
    let mut save_paths = Vec::with_capacity(count);
    for index in 0..count {
        let player = options.local_players[index];
        let device = DeviceId(u64::try_from(index + 1)?);
        let judge = JudgeEngine::new(
            prepared.compiled.chart.clone(),
            prepared.source.rules(),
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(1),
                    early: Duration::from_nanos(options.early),
                    late: Duration::from_nanos(options.late),
                }],
                Duration::from_nanos(options.offset),
            )?,
        )?;
        let bindings =
            BindingMap::from_bindings(options.bindings.iter().map(|(&channel, &key)| Binding {
                device: DeviceSelector::Exact(device),
                physical: PhysicalControlId::keyboard(key),
                game_control: GameControlId(u32::from(channel)),
            }))?;
        let mut sounds = prepared.sounds.clone();
        allocator.remap(&mut sounds)?;
        let path = options
            .record_replay
            .as_deref()
            .map(|base| replay_path(base, player))
            .transpose()?;
        let capture = if path.is_some() {
            Some(LiveReplayCapture::new_at_with_chart_seed(
                &judge,
                HOST,
                ReplayCodecLimits::new(
                    options.replay_max_bytes,
                    options.replay_max_records,
                    4096,
                    beatkernel::input::CodecLimits::new(65536, 32768)?,
                )?,
                Timestamp::from_nanos(options.start_ns),
                options.chart_seed,
            )?)
        } else {
            None
        };
        states.push(PlayerState {
            player,
            capture,
            competition: LiveCompetition::prepare_for_at_with_chart_seed(
                player,
                &competition_options,
                &prepared.source,
                &judge,
                HOST,
                Timestamp::from_nanos(options.start_ns),
                options.chart_seed,
            )?,
            completion: if options.end_ns.is_none() {
                Some(SongCompletion::prepare(
                    &prepared,
                    options.late,
                    options.offset,
                    options.preroll,
                    OUTPUT,
                )?)
            } else {
                None
            },
            score: ScoreSummary::default(),
            last_song: options.song_origin()?,
        });
        save_paths.push(path);
        configs.push(MemberConfig {
            player,
            device: Some(device),
            bindings,
            judge,
            sounds,
        });
    }
    const SLACK: usize = 1024;
    let capacity = AudioLimits::MAX_COMMANDS;
    let (mut producer, consumer) = command_queue(capacity)?;
    let mut bgm = BgmSession(beatkernel_bms_runtime::bgm::BgmFeeder::new(
        beatkernel_bms_runtime::section_start::relative_commands(
            prepared.bgm_commands,
            Timestamp::from_nanos(options.start_ns),
        )?,
        beatkernel_bms_runtime::bgm::BgmConfig {
            output_origin: output_origin(),
            sample_rate: options.format.sample_rate(),
            preroll: Duration::from_nanos(options.preroll),
            lookahead: Duration::from_nanos(options.bgm_lookahead),
            max_pending: capacity - SLACK,
        },
    )?);
    bgm.feed(0, capacity - SLACK, |command| producer.try_push(command))?;
    let mixer = Mixer::new(
        {
            let config = MixerConfig::new(
                options.format,
                OUTPUT,
                Timestamp::ZERO,
                AudioLimits::new(
                    capacity,
                    options.voices,
                    capacity,
                    options.period as usize,
                    capacity,
                )?,
            );
            playback_end.map_or(config, |end| config.with_playback_end_frame(end))
        },
        prepared.bank,
        consumer,
    )?;
    // Every evdev owner precedes the stream. Native output therefore stops and
    // joins before input handles disappear on both normal and exceptional exits.
    let mut inputs = Vec::with_capacity(count);
    let mut native_numbers = HashSet::new();
    for (index, path) in options.local_inputs.iter().enumerate() {
        let input = EvdevDevice::open(path, DeviceId(u64::try_from(index + 1)?), HOST)?;
        if !native_numbers.insert(input.native_device_number()?) {
            return Err("local input paths alias the same physical character device".into());
        }
        inputs.push(input);
    }
    let mut stream = AlsaStream::open(
        AlsaRequest {
            device: options.alsa.clone(),
            format: DeviceFormat::new(
                options.format.sample_rate(),
                options.format.channels(),
                SampleEncoding::Float32,
                None,
            )?,
            buffer_frames: options.buffer,
            period_frames: options.period,
            allow_size_rounding: false,
            monotonic_domain: HOST,
        },
        mixer,
    )?;
    println!(
        "local players={count}; shared requested/applied ALSA={:?}; exact input devices={:?}; one asset bank/BGM/output; independent judges/captures/scores",
        stream.configuration(),
        options.local_inputs
    );
    let mut before_origin = 0u64;
    let outcome = (|| -> Result<()> {
        stream.start()?;
        let mut discipline = PresentationDiscipline::new(
            DisciplineConfig::default(),
            output_origin(),
            HOST,
            song_origin,
        )?;
        let pair = seed(&stream, &mut discipline, &mut bgm, &mut producer)?;
        let host_origin = ClockPoint {
            domain: HOST,
            timestamp: estimated_origin(pair, output_origin())?,
        };
        let mut group = RuntimeGroup::new(
            HOST,
            OUTPUT,
            Transport::new(host_origin.timestamp, song_origin, Rate::NORMAL),
            producer,
            configs,
            4096,
            &reserved,
        )?;
        if let Some(end) = options.end_ns {
            group.set_song_end(Timestamp::from_nanos(end))?;
        }
        let mut merger = InputMerger::new(
            HOST,
            host_origin,
            (1..=count).map(|id| DeviceId(id as u64)).collect(),
            65536,
        )?;
        let deadline = options
            .seconds
            .map(|seconds| Instant::now() + WallDuration::from_secs(seconds));
        let mut last_progress = None;
        let mut backlogged = vec![true; count];
        let mut end_boundary = if let Some(end) = &mut native_end {
            end.observe(stream.last_render_report(), pair)?
        } else {
            None
        };
        let mut end_rendered = false;
        let mut committed_frontier = None;
        let mut keyboard = PauseKeyboard::new();
        let mut paused_boundary: Option<ClockPoint> = None;
        let mut resume_boundary: Option<ClockPoint> = None;
        let mut pause_committed = false;
        let mut pause_lag_reached = false;
        player::publish_pause(PauseState::Running);
        let pump = (|| -> Result<()> {
            while deadline.is_none_or(|deadline| Instant::now() < deadline)
                && !beatkernel_bms_runtime::player::cancelled()
            {
                player::retry_pause_publication();
                if let Some(pair) = observe(&stream, false)? {
                    discipline.observe_clock_pair(pair)?;
                }
                let reference = discipline
                    .latest_pair()
                    .ok_or("local pause requires native clock relation")?;
                let rendered = stream.last_render_report();
                if let Some(end) = &mut native_end {
                    end_rendered |=
                        rendered.is_some_and(|report| report.playback_end_physical_frame.is_some());
                    if let Some(boundary) = end.observe(rendered, reference)? {
                        end_boundary = Some(boundary);
                    }
                }
                if !end_rendered
                    && (pause.phase() == PausePhase::Running || pause_committed)
                    && resume_boundary.is_none()
                    && pause.request(player::pause_requested(), reference)?
                {
                    let desired = pause.phase() == PausePhase::Pausing;
                    group.request_audio_pause(desired);
                    player::publish_pause(if desired {
                        PauseState::Pausing
                    } else {
                        PauseState::Resuming
                    });
                }
                if let Some(boundary) = pause.observe(rendered, reference)? {
                    if boundary.paused {
                        if !end_rendered {
                            group.transport_mut().pause(boundary.host.timestamp)?;
                            paused_boundary = Some(boundary.host);
                            pause_committed = false;
                            pause_lag_reached = false;
                        }
                    } else {
                        group.transport_mut().resume(boundary.host.timestamp)?;
                        resume_boundary = Some(boundary.host);
                        paused_boundary = None;
                        pause_committed = false;
                        discipline = PresentationDiscipline::new(
                            DisciplineConfig::default(),
                            output_origin(),
                            HOST,
                            pause.song_origin_after_pause(song_origin)?,
                        )?;
                        discipline.observe_clock_pair(reference)?;
                    }
                }
                feed_rendered(&mut bgm, stream.last_render_report(), |command| {
                    group.enqueue_audio(command)
                })?;
                if pause.last_render_report().is_none()
                    || matches!(pause.phase(), PausePhase::Pausing | PausePhase::Resuming)
                {
                    std::thread::sleep(WallDuration::from_millis(1));
                    continue;
                }
                backlogged.fill(true);
                // Fair bounded sweeps: acquire all owners before releasing any
                // event or judge deadline. One backlog suppresses the frontier.
                for _ in 0..256 {
                    if backlogged.iter().all(|backlog| !*backlog) {
                        break;
                    }
                    for (index, input) in inputs.iter_mut().enumerate() {
                        if !backlogged[index] {
                            continue;
                        }
                        match input.read_next()? {
                            EvdevItem::WouldBlock => backlogged[index] = false,
                            EvdevItem::Ignored => {}
                            EvdevItem::Dropped => {
                                return Err(
                                    "local evdev SYN_DROPPED; stop/restart whole cohort".into()
                                );
                            }
                            EvdevItem::Resync(_) => {
                                return Err(
                                    "local evdev resync barrier; stop/restart whole cohort".into(),
                                );
                            }
                            EvdevItem::Event(event) => {
                                let host = ClockPoint {
                                    domain: event.meta().clock_domain,
                                    timestamp: event.meta().timestamp,
                                };
                                let received = clock.now()?;
                                discipline.validate_host(received)?;
                                if host.domain != HOST || host.timestamp > received.timestamp {
                                    return Err("local evdev event has invalid host domain/future timestamp".into());
                                }
                                if host.timestamp < host_origin.timestamp {
                                    before_origin = before_origin.saturating_add(1);
                                    continue;
                                }
                                delivery.observe(host, received)?;
                                merger.admit(event, received)?;
                            }
                        }
                    }
                }
                let now = clock.now()?;
                discipline.validate_host(now)?;
                let backlog = backlogged.iter().any(|backlog| *backlog);
                if pause.phase() == PausePhase::Paused && !end_rendered {
                    if !pause_committed && !backlog {
                        let at = paused_boundary.ok_or("local paused boundary unavailable")?;
                        while let Some(event) = merger.pop_ready(at)? {
                            let host = ClockPoint {
                                domain: event.meta().clock_domain,
                                timestamp: event.meta().timestamp,
                            };
                            discipline.validate_host(host)?;
                            if host.timestamp >= at.timestamp {
                                keyboard.observe_paused(event)?;
                            } else if keyboard.accept(&event)? {
                                process_event(event, &mut group, &pause, &stream, &mut states)?;
                            }
                        }
                        advance_group(at, &mut group, &pause, &stream, &mut states)?;
                        merger.commit(at)?;
                        committed_frontier = Some(at);
                        pause_committed = true;
                        player::publish_pause(PauseState::Paused);
                    }
                    if pause_committed {
                        if !pause_lag_reached {
                            pause_lag_reached = lag_reaches(
                                now,
                                paused_boundary.ok_or("local paused boundary unavailable")?,
                                options.advance_lag,
                            )?;
                        }
                        if pause_lag_reached {
                            if let Some(frontier) =
                                merger.watermark(now, options.advance_lag, backlog)?
                            {
                                while let Some(event) = merger.pop_ready(frontier)? {
                                    keyboard.observe_paused(event)?;
                                }
                            }
                        }
                    }
                    std::thread::sleep(WallDuration::from_millis(1));
                    continue;
                }
                if let Some(at) = resume_boundary {
                    if !lag_reaches(now, at, options.advance_lag)? {
                        std::thread::sleep(WallDuration::from_millis(1));
                        continue;
                    }
                }
                let frontier = merger.watermark(now, options.advance_lag, backlog)?;
                if resume_boundary.is_some_and(|at| {
                    frontier.is_none_or(|frontier| frontier.timestamp < at.timestamp)
                }) {
                    std::thread::sleep(WallDuration::from_millis(1));
                    continue;
                }
                let was_resuming = resume_boundary.is_some();
                if !was_resuming {
                    update_discipline(&mut discipline, now, &mut group)?;
                }
                if let Some(frontier) = frontier {
                    while let Some(event) = merger.pop_ready(frontier)? {
                        let host = ClockPoint {
                            domain: event.meta().clock_domain,
                            timestamp: event.meta().timestamp,
                        };
                        discipline.validate_host(host)?;
                        if let Some(at) = resume_boundary {
                            if host.timestamp < at.timestamp {
                                keyboard.observe_paused(event)?;
                                continue;
                            }
                            reconcile_resume(
                                &mut keyboard,
                                at,
                                &mut group,
                                &pause,
                                &stream,
                                &mut states,
                            )?;
                            resume_boundary = None;
                            player::publish_pause(PauseState::Running);
                        }
                        // Preserve paused-prefix levels and resume releases first.
                        // Acquisition/order remain validated even when gameplay ends.
                        if end_boundary
                            .is_some_and(|boundary| host.timestamp >= boundary.host.timestamp)
                        {
                            continue;
                        }
                        if keyboard.accept(&event)? {
                            process_event(event, &mut group, &pause, &stream, &mut states)?;
                        }
                    }
                    if let Some(at) = resume_boundary.take() {
                        reconcile_resume(
                            &mut keyboard,
                            at,
                            &mut group,
                            &pause,
                            &stream,
                            &mut states,
                        )?;
                        player::publish_pause(PauseState::Running);
                    }
                    if was_resuming {
                        update_discipline(&mut discipline, now, &mut group)?;
                    }
                    advance_group(frontier, &mut group, &pause, &stream, &mut states)?;
                    merger.commit(frontier)?;
                    committed_frontier = Some(frontier);
                    let second = states[0].last_song.as_nanos().div_euclid(1_000_000_000);
                    if last_progress != Some(second) {
                        println!(
                            "shared logical song={}ns; pending merged input={}",
                            states[0].last_song.as_nanos(),
                            merger.pending()
                        );
                        for state in &states {
                            println!("player{} score={:?}", state.player.0, state.score);
                        }
                        last_progress = Some(second);
                    }
                }
                if finite_cohort_done(
                    options.end_ns,
                    end_boundary.map(|boundary| boundary.host),
                    committed_frontier,
                    &states,
                    backlog,
                    resume_boundary.is_some(),
                ) {
                    player::publish_section_end(Timestamp::from_nanos(
                        options.end_ns.expect("finite endpoint admitted"),
                    ));
                    println!(
                        "all local finite prefixes complete: native endpoint presented and committed input frontier drained; remaining notes are not forced complete"
                    );
                    break;
                }
                let mut finished = options.end_ns.is_none();
                for state in &mut states {
                    if let Some(completion) = &mut state.completion {
                        let judge = group
                            .member_judge(state.player)
                            .ok_or("local judge unavailable")?;
                        finished &= completion.observe(
                            judge,
                            state.last_song,
                            bgm.report(),
                            stream.last_render_report(),
                            discipline.latest_pair().map(|pair| pair.source),
                        )?;
                    }
                }
                if finished {
                    println!(
                        "all local players complete: independent terminal judging and shared native output drain"
                    );
                    break;
                }
                std::thread::sleep(WallDuration::from_millis(1));
            }
            Ok(())
        })();
        for state in &states {
            if let Some(telemetry) = group.member_telemetry(state.player) {
                println!(
                    "player{} final score={:?}; processing={:?}; counters={:?}",
                    state.player.0,
                    state.score,
                    telemetry.processing(),
                    telemetry.counters()
                );
            }
        }
        pump
    })();
    let timing = stream.timing_snapshot();
    let stop = stream.stop();
    println!(
        "shared final ALSA={:?}; timing={timing:?}; last mixer={:?}; pre-origin ignored={before_origin}; physical delivery unverified",
        stream.snapshot(),
        stream.last_render_report()
    );
    for (index, input) in inputs.iter().enumerate() {
        println!(
            "player{} final evdev counters={:?}",
            options.local_players[index].0,
            input.counters()
        );
    }
    if let Err(error) = &stop {
        eprintln!("local ALSA stop/join error: {error}");
    }
    drop(inputs);
    let failed = outcome.is_err() || stop.is_err();
    let mut saves = Vec::new();
    for (mut state, path) in states.into_iter().zip(save_paths) {
        if let Some(competition) = state.competition.as_mut() {
            competition.finish();
        }
        if let Err(error) = save_capture(state.capture, path.as_deref(), failed) {
            let detail = format!(
                "player{} replay save after cleanup: {error}",
                state.player.0
            );
            eprintln!("{detail}");
            saves.push(detail);
        }
    }
    outcome?;
    stop?;
    if !saves.is_empty() {
        return Err(saves.join("; ").into());
    }
    Ok(())
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::input::{ButtonEvent, ButtonState, EventMeta, PhysicalInputEvent};
    fn host(ns: i64) -> ClockPoint {
        ClockPoint {
            domain: HOST,
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn button(device: u64, state: ButtonState, ns: i64, sequence: u64) -> PhysicalInputEvent {
        PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(device), host(ns), sequence),
            control: PhysicalControlId::keyboard(4),
            state,
        })
    }
    #[test]
    fn exact_pause_commit_waits_for_lag_without_hiding_later_merger_regression() {
        let mut merger =
            InputMerger::new(HOST, host(0), vec![DeviceId(1), DeviceId(2)], 4).unwrap();
        merger.commit(host(10_000_000)).unwrap();
        assert!(!lag_reaches(host(10_500_000), host(10_000_000), 2_000_000).unwrap());
        assert!(lag_reaches(host(12_000_000), host(10_000_000), 2_000_000).unwrap());
        assert_eq!(
            merger
                .watermark(host(12_000_000), 2_000_000, false)
                .unwrap(),
            Some(host(10_000_000))
        );
        assert!(
            merger
                .watermark(host(11_000_000), 2_000_000, false)
                .is_err()
        );
        assert!(!lag_reaches(host(12_000_000), host(11_000_000), 2_000_000).unwrap());
        assert!(lag_reaches(host(13_000_000), host(11_000_000), 2_000_000).unwrap());
        assert!(!lag_reaches(host(i64::MIN), host(i64::MIN), 1).unwrap());
        assert!(lag_reaches(host(10), host(10), -1).is_err());
        assert!(
            lag_reaches(
                host(10),
                ClockPoint {
                    domain: OUTPUT,
                    timestamp: Timestamp::ZERO
                },
                0
            )
            .is_err()
        );
    }
    #[test]
    fn shared_pause_frontier_drains_global_order_including_boundary_without_moving_commit() {
        let mut merger =
            InputMerger::new(HOST, host(0), (1..=4).map(DeviceId).collect(), 16).unwrap();
        let mut keyboard = PauseKeyboard::new();
        for event in [
            button(4, ButtonState::Down, 9, 1),
            button(2, ButtonState::Down, 9, 1),
            button(1, ButtonState::Down, 10, 1),
            button(3, ButtonState::Down, 11, 1),
        ] {
            merger.admit(event, host(20)).unwrap();
        }
        assert_eq!(merger.watermark(host(20), 2, true).unwrap(), None);
        let mut judged = Vec::new();
        while let Some(event) = merger.pop_ready(host(10)).unwrap() {
            if event.meta().timestamp >= host(10).timestamp {
                keyboard.observe_paused(event).unwrap();
            } else if keyboard.accept(&event).unwrap() {
                judged.push(event.meta().source);
            }
        }
        assert_eq!(judged, vec![DeviceId(2), DeviceId(4)]);
        merger.commit(host(10)).unwrap();
        let safe = merger.watermark(host(20), 2, false).unwrap().unwrap();
        while let Some(event) = merger.pop_ready(safe).unwrap() {
            keyboard.observe_paused(event).unwrap();
        }
        // Paused draining consumes levels but leaves the committed judge
        // frontier at the actual pause boundary, so later paused input is legal.
        merger
            .admit(button(3, ButtonState::Up, 12, 2), host(21))
            .unwrap();
        assert_eq!(merger.pending(), 1);
        assert!(
            !keyboard
                .accept(&button(1, ButtonState::Repeat, 21, 2))
                .unwrap()
        );
    }
    #[test]
    fn resume_keeps_lag_and_reconciles_all_devices_before_postboundary_inputs() {
        let mut merger =
            InputMerger::new(HOST, host(0), vec![DeviceId(1), DeviceId(2)], 16).unwrap();
        let mut keyboard = PauseKeyboard::new();
        keyboard
            .accept(&button(1, ButtonState::Down, 1, 1))
            .unwrap();
        keyboard
            .accept(&button(2, ButtonState::Down, 1, 1))
            .unwrap();
        merger.commit(host(10)).unwrap();
        for event in [
            button(2, ButtonState::Up, 19, 2),
            button(1, ButtonState::Up, 18, 2),
            button(1, ButtonState::Down, 21, 3),
        ] {
            merger.admit(event, host(30)).unwrap();
        }
        assert_eq!(
            merger.watermark(host(21), 2, false).unwrap(),
            Some(host(19))
        );
        assert_eq!(merger.watermark(host(30), 2, true).unwrap(), None);
        let frontier = merger.watermark(host(30), 2, false).unwrap().unwrap();
        let mut at = Some(host(20));
        let mut admitted = Vec::new();
        while let Some(event) = merger.pop_ready(frontier).unwrap() {
            if let Some(boundary) = at {
                if event.meta().timestamp < boundary.timestamp {
                    keyboard.observe_paused(event).unwrap();
                    continue;
                }
                admitted.extend(keyboard.resume(boundary).unwrap());
                at = None;
            }
            if keyboard.accept(&event).unwrap() {
                admitted.push(event);
            }
        }
        assert!(at.is_none());
        assert_eq!(admitted.len(), 3);
        assert_eq!(admitted[0].meta().source, DeviceId(1));
        assert_eq!(admitted[1].meta().source, DeviceId(2));
        assert_eq!(admitted[0].meta().timestamp, host(20).timestamp);
        assert_eq!(admitted[0].meta().original_clock_point, Some(host(18)));
        assert_eq!(admitted[2].meta().timestamp, host(21).timestamp);
        merger.commit(frontier).unwrap();
        assert!(
            merger
                .admit(button(2, ButtonState::Down, 27, 3), host(31))
                .is_err()
        );
    }
    fn finite_states(ids: &[PlayerId], song: i64) -> Vec<PlayerState> {
        ids.iter()
            .map(|&player| PlayerState {
                player,
                capture: None,
                competition: None,
                completion: None,
                score: ScoreSummary::default(),
                last_song: Timestamp::from_nanos(song),
            })
            .collect()
    }
    #[test]
    fn finite_cohort_needs_every_member_actual_commit_native_ack_and_all_source_drain() {
        for count in [2, 3, 4, 64] {
            let mut ids: Vec<_> = (0..count).map(|i| PlayerId(1000 + i as u32 * 7)).collect();
            *ids.last_mut().unwrap() = PlayerId(u32::MAX);
            let mut states = finite_states(&ids, 10);
            assert!(finite_cohort_done(
                Some(10),
                Some(host(20)),
                Some(host(20)),
                &states,
                false,
                false
            ));
            for (boundary, committed, backlog, resuming) in [
                (None, Some(host(20)), false, false),
                (Some(host(20)), None, false, false),
                (Some(host(20)), Some(host(19)), false, false),
                (Some(host(20)), Some(host(20)), true, false),
                (Some(host(20)), Some(host(20)), false, true),
            ] {
                assert!(!finite_cohort_done(
                    Some(10),
                    boundary,
                    committed,
                    &states,
                    backlog,
                    resuming
                ));
            }
            states.last_mut().unwrap().last_song = Timestamp::from_nanos(9);
            assert!(!finite_cohort_done(
                Some(10),
                Some(host(20)),
                Some(host(21)),
                &states,
                false,
                false
            ));
            states.last_mut().unwrap().last_song = Timestamp::from_nanos(10);
            assert!(!finite_cohort_done(
                None,
                Some(host(20)),
                Some(host(21)),
                &states,
                false,
                false
            ));
            assert_eq!(states.last().unwrap().player, PlayerId(u32::MAX));
            assert!(states.iter().all(|state| state.completion.is_none()));
        }
    }
    #[test]
    fn finite_global_pop_keeps_preboundary_order_and_only_real_commit_finishes() {
        let states = finite_states(
            &[
                PlayerId(7),
                PlayerId(1000),
                PlayerId(u32::MAX),
                PlayerId(42),
            ],
            10,
        );
        let mut merger =
            InputMerger::new(HOST, host(0), (1..=4).map(DeviceId).collect(), 16).unwrap();
        for event in [
            button(4, ButtonState::Down, 9, 1),
            button(2, ButtonState::Down, 9, 1),
            button(1, ButtonState::Down, 10, 1),
            button(3, ButtonState::Down, 11, 1),
            button(2, ButtonState::Up, 21, 2),
        ] {
            merger.admit(event, host(30)).unwrap();
        }
        assert_eq!(merger.watermark(host(20), 2, true).unwrap(), None);
        let frontier = merger.watermark(host(20), 2, false).unwrap().unwrap();
        let mut gameplay = Vec::new();
        let mut acquired = Vec::new();
        while let Some(event) = merger.pop_ready(frontier).unwrap() {
            acquired.push(event.meta().source);
            if event.meta().timestamp < host(10).timestamp {
                gameplay.push(event.meta().source);
            }
        }
        assert_eq!(gameplay, vec![DeviceId(2), DeviceId(4)]);
        assert_eq!(
            acquired,
            vec![DeviceId(2), DeviceId(4), DeviceId(1), DeviceId(3)]
        );
        assert!(!finite_cohort_done(
            Some(10),
            Some(host(10)),
            None,
            &states,
            false,
            false
        ));
        merger.commit(frontier).unwrap();
        assert!(finite_cohort_done(
            Some(10),
            Some(host(10)),
            Some(frontier),
            &states,
            false,
            false
        ));
        // Future post-end acquisition is retained by the bounded merger until
        // owner cleanup; it neither blocks this prefix nor invents gameplay.
        assert_eq!(merger.pending(), 1);
        assert!(
            merger
                .admit(button(1, ButtonState::Up, 17, 2), host(30))
                .is_err()
        );
    }
    #[test]
    fn local_replay_paths_preserve_parent_and_use_distinct_player_suffixes() {
        assert_eq!(
            replay_path(Path::new("records/run.bkr"), PlayerId(1)).unwrap(),
            PathBuf::from("records/run.p1.bkr")
        );
        assert_eq!(
            replay_path(Path::new("records/run.bkr"), PlayerId(64)).unwrap(),
            PathBuf::from("records/run.p64.bkr")
        );
        assert!(replay_path(Path::new("/"), PlayerId(1)).is_err());
        assert!(replay_path(Path::new("run.bkr"), PlayerId(0)).is_err());
        for id in [7, 1000, u32::MAX] {
            assert_eq!(
                replay_path(Path::new("run.bkr"), PlayerId(id)).unwrap(),
                PathBuf::from(format!("run.p{id}.bkr"))
            );
        }
    }
    #[test]
    fn unsupported_local_modes_reject_before_resource_preparation() {
        assert!(admit_mode(2, false).is_ok());
        assert!(admit_mode(64, false).is_ok());
        for (count, network) in [(1, false), (65, false), (2, true)] {
            assert!(admit_mode(count, network).is_err());
        }
    }
}
