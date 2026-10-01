//! Actual macOS local cohort: one IOHID owner, one shared CoreAudio output.
use super::native::{ExplicitDomains, HOST, M_NATIVE, OUTPUT, observe, output_origin};
use super::*;
use beatkernel::{
    audio::{AudioCommand, CommandProducer, Mixer, MixerConfig, PcmLimits, command_queue},
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
    load_prepared,
    local_input::InputMerger,
    local_players::{MAX_LOCAL_PLAYERS, PlayerId},
    local_runtime::{InputResult, MemberConfig, PlayerReport, RuntimeGroup, VoiceAllocator},
    playback_pause::{NativePause, PauseKeyboard, PausePhase},
    player::{self, PauseState},
    replay_capture::LiveReplayCapture,
};
use beatkernel_platform::{
    audio::presentation::discipline::{DisciplineConfig, DisciplineUpdate, PresentationDiscipline},
    macos::{
        audio::{CoreAudioRequest, CoreAudioStream},
        clock::MachClock,
        input::{HidCounters, HidInput},
    },
};
use std::{
    path::Path,
    time::{Duration as WallDuration, Instant},
};

struct PlayerState {
    player: PlayerId,
    capture: Option<LiveReplayCapture>,
    competition: Option<LiveCompetition>,
    completion: SongCompletion,
    score: ScoreSummary,
    last_song: Timestamp,
}

fn admit_mode(count: usize, network: bool) -> Result<()> {
    if !(2..=MAX_LOCAL_PLAYERS).contains(&count) {
        return Err("macOS local play requires 2..64 exact registry assignments".into());
    }
    if network {
        return Err("network competition currently supports one local participant only".into());
    }
    Ok(())
}

/// Missing attachments may arrive during bounded preparation; ambiguity,
/// incapable attachments and identity aliases are terminal setup errors.
fn resolve_registries(
    requested: &[(PlayerId, u64)],
    attached: &[(Option<u64>, DeviceId, bool)],
) -> Result<Option<Vec<DeviceId>>> {
    let mut players = HashSet::new();
    let mut registries = HashSet::new();
    let mut ids = HashSet::new();
    let mut selected = Vec::with_capacity(requested.len());
    let mut missing = false;
    for (player, registry) in requested {
        if player.0 == 0
            || *registry == 0
            || !players.insert(*player)
            || !registries.insert(*registry)
        {
            return Err("invalid or duplicate local player/IORegistry assignment".into());
        }
        let mut matches = attached.iter().filter(|(id, _, _)| *id == Some(*registry));
        let Some((_, id, button)) = matches.next() else {
            missing = true;
            continue;
        };
        if matches.next().is_some() {
            return Err("ambiguous local IORegistry attachment".into());
        }
        if !button || id.0 == 0 || !ids.insert(*id) {
            return Err("incapable or aliased local IORegistry attachment".into());
        }
        selected.push(*id);
    }
    Ok(if missing { None } else { Some(selected) })
}
fn check_counters(counters: HidCounters) -> Result<()> {
    if counters.queue_full != 0
        || counters.unsupported != 0
        || counters.reports_queue_full != 0
        || counters.reports_oversized != 0
        || counters.reports_invalid != 0
        || counters.reports_allocation_failed != 0
        || counters.reports_timestamp_failed != 0
    {
        return Err(format!("IOHID acquisition loss; restart whole cohort: {counters:?}").into());
    }
    Ok(())
}
fn current_ids(input: &HidInput, requested: &[(PlayerId, u64)]) -> Result<Option<Vec<DeviceId>>> {
    check_counters(input.counters())?;
    let attached: Vec<_> = input
        .devices()
        .iter()
        .map(|device| {
            (
                device.registry_entry,
                device.descriptor.runtime_id,
                device.descriptor.capabilities.button,
            )
        })
        .collect();
    resolve_registries(requested, &attached)
}
fn select_group(input: &mut HidInput, requested: &[(PlayerId, u64)]) -> Result<Vec<DeviceId>> {
    let deadline = Instant::now() + WallDuration::from_secs(2);
    loop {
        if let Some(ids) = current_ids(input, requested)? {
            return Ok(ids);
        }
        if Instant::now() >= deadline {
            return Err("local IORegistry assignments unavailable within two seconds".into());
        }
        input.poll(WallDuration::from_millis(1))?;
    }
}
fn check_group(
    input: &HidInput,
    requested: &[(PlayerId, u64)],
    selected: &[DeviceId],
) -> Result<()> {
    if current_ids(input, requested)?.as_deref() != Some(selected) {
        return Err(
            "assigned IORegistry attachment retired/reconnected; restart whole cohort".into(),
        );
    }
    Ok(())
}
/// Every seed poll validates the full roster, never just one selected member.
fn seed_group(
    audio: &CoreAudioStream,
    input: &mut HidInput,
    clock: &MachClock,
    requested: &[(PlayerId, u64)],
    selected: &[DeviceId],
    discipline: &mut PresentationDiscipline,
    bgm: &mut BgmSession,
    producer: &mut CommandProducer,
) -> Result<ClockPair> {
    let deadline = Instant::now() + WallDuration::from_secs(2);
    while Instant::now() < deadline {
        input.poll(WallDuration::from_millis(1))?;
        check_group(input, requested, selected)?;
        feed_rendered(bgm, audio.last_render_report(), |command| {
            producer.try_push(command)
        })?;
        if let Some(pair) = observe(audio, clock)? {
            discipline.observe_clock_pair(pair)?;
            return Ok(pair);
        }
    }
    Err("no valid native CoreAudio group presentation seed within two seconds".into())
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

fn playback_schedule(pause: &NativePause, stream: &CoreAudioStream) -> Result<ClockPoint> {
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
    stream: &CoreAudioStream,
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
    stream: &CoreAudioStream,
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
    stream: &CoreAudioStream,
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
        options.local_players.len(),
        competition_options.network.is_some(),
    )?;
    let count = options.local_players.len();
    let song_origin = options.song_origin()?;
    let clock = MachClock::new(M_NATIVE, HOST)?;
    // Declared before native owners so delivery observations report after cleanup.
    let mut delivery = DeliverySession(beatkernel::telemetry::InputDeliveryTelemetry::new(
        4096, HOST,
    )?);
    let prepared = load_prepared(
        &options.chart,
        options.format,
        PcmLimits::new(64 * 1024 * 1024, 256 * 1024 * 1024, 1295)?,
        if options.mono_stereo {
            ChannelPolicy::MonoToStereo
        } else {
            ChannelPolicy::Exact
        },
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
        &options
            .local_players
            .iter()
            .map(|(player, _)| *player)
            .collect::<Vec<_>>(),
    )?;
    if beatkernel_bms_runtime::player::cancelled() {
        return Ok(());
    }
    // Input precedes output, retaining owner-thread close order on early exits.
    let mut input = HidInput::open(clock, DeviceId(1), 65536)?;
    let selected = match select_group(&mut input, &options.local_players) {
        Ok(selected) => selected,
        Err(error) => {
            let mut failures = vec![format!("local IORegistry selection: {error}")];
            if let Err(close) = input.close() {
                failures.push(format!("IOHID close after selection failure: {close}"));
            }
            return Err(failures.join("; ").into());
        }
    };
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
        let player = options.local_players[index].0;
        let device = selected[index];
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
            Some(LiveReplayCapture::new_at(
                &judge,
                HOST,
                ReplayCodecLimits::new(
                    options.replay_max_bytes,
                    options.replay_max_records,
                    4096,
                    beatkernel::input::CodecLimits::new(65536, 32768)?,
                )?,
                Timestamp::from_nanos(options.start_ns),
            )?)
        } else {
            None
        };
        states.push(PlayerState {
            player,
            capture,
            competition: LiveCompetition::prepare_for_at(
                player,
                &competition_options,
                &prepared.source,
                &judge,
                HOST,
                Timestamp::from_nanos(options.start_ns),
            )?,
            completion: SongCompletion::prepare(
                &prepared,
                options.late,
                options.offset,
                options.preroll,
                OUTPUT,
            )?,
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
        MixerConfig::new(
            options.format,
            OUTPUT,
            Timestamp::ZERO,
            AudioLimits::new(
                capacity,
                options.voices,
                capacity,
                options.buffer as usize,
                capacity,
            )?,
        ),
        prepared.bank,
        consumer,
    )?;
    let mut stream = match CoreAudioStream::open(
        CoreAudioRequest {
            device: options.device,
            format: options.format,
            buffer_frames: options.buffer,
        },
        clock,
        mixer,
    ) {
        Ok(stream) => stream,
        Err(error) => {
            let mut failures = vec![format!("CoreAudio open: {error}")];
            if let Err(close) = input.close() {
                failures.push(format!("IOHID close after audio open failure: {close}"));
            }
            for state in &mut states {
                if let Some(competition) = state.competition.as_mut() {
                    competition.finish();
                }
            }
            return Err(failures.join("; ").into());
        }
    };
    println!(
        "macOS local players={count}; shared requested/applied CoreAudio={:?}; independent judges/captures/scores",
        stream.configuration()
    );
    let mut before_origin = 0u64;
    let outcome = (|| -> Result<()> {
        check_group(&input, &options.local_players, &selected)?;
        stream.start()?;
        let mut discipline = PresentationDiscipline::new(
            DisciplineConfig::default(),
            output_origin(),
            HOST,
            song_origin,
        )?;
        let pair = seed_group(
            &stream,
            &mut input,
            &clock,
            &options.local_players,
            &selected,
            &mut discipline,
            &mut bgm,
            &mut producer,
        )?;
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
        let mut merger = InputMerger::new(HOST, host_origin, selected.clone(), 65536)?;
        let deadline = options
            .seconds
            .map(|seconds| Instant::now() + WallDuration::from_secs(seconds));
        let mut last_progress = None;
        let mut pause = NativePause::new(output_origin(), HOST, options.format.sample_rate())?;
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
                input.poll(WallDuration::from_millis(1))?;
                check_group(&input, &options.local_players, &selected)?;
                if let Some(pair) = observe(&stream, &clock)? {
                    discipline.observe_clock_pair(pair)?;
                }
                let reference = discipline
                    .latest_pair()
                    .ok_or("local pause requires native clock relation")?;
                if (pause.phase() == PausePhase::Running || pause_committed)
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
                if let Some(boundary) = pause.observe(stream.last_render_report(), reference)? {
                    if boundary.paused {
                        group.transport_mut().pause(boundary.host.timestamp)?;
                        paused_boundary = Some(boundary.host);
                        pause_committed = false;
                        pause_lag_reached = false;
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
                let mut backlog = true;
                // Conservative true after exactly256 popped records: the next
                // bounded iteration confirms empty before allowing deadlines.
                for _ in 0..256 {
                    let Some(sample) = input.pop() else {
                        backlog = false;
                        break;
                    };
                    let event = sample.event;
                    if !selected.contains(&event.meta().source) {
                        continue;
                    }
                    let host = ClockPoint {
                        domain: event.meta().clock_domain,
                        timestamp: event.meta().timestamp,
                    };
                    let received = clock.sample()?.normalized;
                    discipline.validate_host(received)?;
                    if host.domain != HOST || host.timestamp > received.timestamp {
                        return Err("local IOHID has invalid HOST domain/future timestamp".into());
                    }
                    if host.timestamp < host_origin.timestamp {
                        before_origin = before_origin
                            .checked_add(1)
                            .ok_or("pre-origin counter overflow")?;
                        continue;
                    }
                    discipline.validate_host(host)?;
                    delivery.observe(host, received)?;
                    merger.admit(event, received)?;
                }
                let now = clock.sample()?.normalized;
                discipline.validate_host(now)?;
                if now.timestamp < host_origin.timestamp {
                    continue;
                }
                if pause.phase() == PausePhase::Paused {
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
                let mut finished = true;
                for state in &mut states {
                    let judge = group
                        .member_judge(state.player)
                        .ok_or("local judge unavailable")?;
                    finished &= state.completion.observe(
                        judge,
                        state.last_song,
                        bgm.report(),
                        stream.last_render_report(),
                        discipline.latest_pair().map(|pair| pair.source),
                    )?;
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
    // Stop native audio callbacks before unregistering HID and writing files.
    let stop = stream.stop();
    let close = input.close();
    println!(
        "shared final CoreAudio={:?}; HID counters={:?}; pre-origin ignored={before_origin}; physical delivery unverified",
        stream.snapshot(),
        input.counters()
    );
    if let Err(error) = &stop {
        eprintln!("CoreAudio stop error: {error}");
    }
    if let Err(error) = &close {
        eprintln!("IOHID close error: {error}");
    }
    let failed = outcome.is_err() || stop.is_err() || close.is_err();
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
    let mut failures = saves;
    if let Err(error) = outcome {
        failures.push(format!("local session: {error}"));
    }
    if let Err(error) = stop {
        failures.push(format!("output cleanup: {error}"));
    }
    if let Err(error) = close {
        failures.push(format!("input cleanup: {error}"));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; ").into())
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::input::{
        BackendId, ButtonEvent, ButtonState, EventMeta, NativeEventMeta, PhysicalInputEvent,
    };
    fn host(ns: i64) -> ClockPoint {
        ClockPoint {
            domain: HOST,
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn paused_button(
        device: DeviceId,
        key: u16,
        state: ButtonState,
        ns: i64,
        sequence: u64,
    ) -> PhysicalInputEvent {
        let mut meta = EventMeta::new(device, host(ns), sequence);
        meta.native = Some(NativeEventMeta {
            backend: BackendId(7),
            code: Some(key as u32),
            timestamp: Some(ClockPoint {
                domain: M_NATIVE,
                timestamp: Timestamp::from_nanos(ns * 100),
            }),
        });
        PhysicalInputEvent::Button(ButtonEvent {
            meta,
            control: PhysicalControlId::keyboard(key),
            state,
        })
    }
    #[test]
    fn four_fresh_registry_devices_keep_independent_paused_levels_and_native_release_provenance() {
        let requested = [
            (PlayerId(7), 900),
            (PlayerId(u32::MAX), 901),
            (PlayerId(1000), 902),
            (PlayerId(3), 903),
        ];
        let attached = [
            (Some(900), DeviceId(11), true),
            (Some(901), DeviceId(22), true),
            (Some(902), DeviceId(33), true),
            (Some(903), DeviceId(44), true),
        ];
        let ids = resolve_registries(&requested, &attached).unwrap().unwrap();
        let mut keyboard = PauseKeyboard::new();
        let mut merger = InputMerger::new(HOST, host(0), ids.clone(), 16).unwrap();
        for &device in &ids {
            assert!(
                keyboard
                    .accept(&paused_button(device, 4, ButtonState::Down, 1, 1))
                    .unwrap()
            );
        }
        merger.commit(host(10)).unwrap();
        let release = paused_button(DeviceId(11), 4, ButtonState::Up, 19, 2);
        for event in [
            release.clone(),
            paused_button(DeviceId(33), 4, ButtonState::Up, 18, 2),
            paused_button(DeviceId(44), 5, ButtonState::Down, 19, 2),
            paused_button(DeviceId(11), 4, ButtonState::Down, 21, 3),
        ] {
            merger.admit(event, host(30)).unwrap();
        }
        assert_eq!(merger.watermark(host(30), 2, true).unwrap(), None);
        let frontier = merger.watermark(host(30), 2, false).unwrap().unwrap();
        let mut boundary = Some(host(20));
        let mut admitted = Vec::new();
        while let Some(event) = merger.pop_ready(frontier).unwrap() {
            if let Some(at) = boundary {
                if event.meta().timestamp < at.timestamp {
                    keyboard.observe_paused(event).unwrap();
                    continue;
                }
                admitted.extend(keyboard.resume(at).unwrap());
                boundary = None;
            }
            if keyboard.accept(&event).unwrap() {
                admitted.push(event);
            }
        }
        assert_eq!(admitted.len(), 3);
        assert_eq!(admitted[0].meta().source, DeviceId(11));
        assert_eq!(admitted[1].meta().source, DeviceId(33));
        assert_eq!(admitted[0].meta().native, release.meta().native);
        assert_eq!(admitted[0].meta().original_clock_point, Some(host(19)));
        assert_eq!(admitted[0].meta().timestamp, host(20).timestamp);
        assert_eq!(admitted[2].meta().timestamp, host(21).timestamp);
        assert!(
            !keyboard
                .accept(&paused_button(DeviceId(44), 5, ButtonState::Repeat, 31, 3))
                .unwrap()
        );
        assert!(
            keyboard
                .accept(&paused_button(DeviceId(22), 4, ButtonState::Repeat, 31, 2))
                .unwrap()
        );
        merger.commit(frontier).unwrap();
    }
    #[test]
    fn actual_pause_commit_and_short_resume_wait_for_exact_safe_lag_frontiers() {
        let mut merger =
            InputMerger::new(HOST, host(0), vec![DeviceId(11), DeviceId(22)], 4).unwrap();
        merger.commit(host(10_000_000)).unwrap();
        assert!(!lag_reaches(host(10_500_000), host(10_000_000), 2_000_000).unwrap());
        assert!(lag_reaches(host(12_000_000), host(10_000_000), 2_000_000).unwrap());
        assert_eq!(
            merger
                .watermark(host(12_000_000), 2_000_000, false)
                .unwrap(),
            Some(host(10_000_000))
        );
        assert!(!lag_reaches(host(12_000_000), host(11_000_000), 2_000_000).unwrap());
        assert!(lag_reaches(host(13_000_000), host(11_000_000), 2_000_000).unwrap());
        assert!(
            merger
                .watermark(host(11_000_000), 2_000_000, false)
                .is_err()
        );
        assert!(!lag_reaches(host(i64::MIN), host(i64::MIN), 1).unwrap());
        assert!(
            lag_reaches(
                host(0),
                ClockPoint {
                    domain: OUTPUT,
                    timestamp: Timestamp::ZERO
                },
                0
            )
            .is_err()
        );
        assert!(lag_reaches(host(0), host(0), 1_000_000_001).is_err());
    }
    #[test]
    fn four_registry_assignments_resolve_fresh_ids_and_fence_aliases() {
        let requested = [
            (PlayerId(7), 900),
            (PlayerId(1000), 901),
            (PlayerId(u32::MAX), 902),
            (PlayerId(3), 903),
        ];
        let attached = [
            (Some(900), DeviceId(11), true),
            (Some(901), DeviceId(22), true),
            (Some(902), DeviceId(33), true),
            (Some(903), DeviceId(44), true),
        ];
        assert_eq!(
            resolve_registries(&requested, &attached).unwrap(),
            Some(vec![DeviceId(11), DeviceId(22), DeviceId(33), DeviceId(44)])
        );
        assert_eq!(
            resolve_registries(&requested, &attached[..3]).unwrap(),
            None
        );
        let mut invalid = attached;
        invalid[3].1 = DeviceId(11);
        assert!(resolve_registries(&requested, &invalid).is_err());
        invalid = attached;
        invalid[3].2 = false;
        assert!(resolve_registries(&requested, &invalid).is_err());
        let mut ambiguous = attached.to_vec();
        ambiguous.push((Some(900), DeviceId(55), true));
        assert!(resolve_registries(&requested, &ambiguous).is_err());
        let mut duplicate = requested;
        duplicate[3].1 = 900;
        assert!(resolve_registries(&duplicate, &attached).is_err());
    }
    #[test]
    fn loss_counters_fence_cohort_but_unassigned_removal_is_not_itself_loss() {
        assert!(
            check_counters(HidCounters {
                removed: 1,
                ..Default::default()
            })
            .is_ok()
        );
        for counters in [
            HidCounters {
                queue_full: 1,
                ..Default::default()
            },
            HidCounters {
                unsupported: 1,
                ..Default::default()
            },
            HidCounters {
                reports_timestamp_failed: 1,
                ..Default::default()
            },
            HidCounters {
                reports_allocation_failed: 1,
                ..Default::default()
            },
        ] {
            assert!(check_counters(counters).is_err());
        }
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
