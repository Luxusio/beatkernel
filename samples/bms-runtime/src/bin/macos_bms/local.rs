//! Actual macOS local cohort: one IOHID owner, one shared CoreAudio output.
use super::native::{ExplicitDomains, HOST, M_NATIVE, OUTPUT, observe, output_origin, schedule};
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

pub(super) fn run(options: Options, competition_options: CompetitionOptions) -> Result<()> {
    admit_mode(
        options.local_players.len(),
        competition_options.network.is_some(),
    )?;
    let count = options.local_players.len();
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
            Some(LiveReplayCapture::new(
                &judge,
                HOST,
                ReplayCodecLimits::new(
                    options.replay_max_bytes,
                    options.replay_max_records,
                    4096,
                    beatkernel::input::CodecLimits::new(65536, 32768)?,
                )?,
            )?)
        } else {
            None
        };
        states.push(PlayerState {
            player,
            capture,
            competition: LiveCompetition::prepare_for(
                player,
                &competition_options,
                &prepared.source,
                &judge,
                HOST,
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
            options.song_origin()?,
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
            Transport::new(host_origin.timestamp, options.song_origin()?, Rate::NORMAL),
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
        let pump = (|| -> Result<()> {
            while deadline.is_none_or(|deadline| Instant::now() < deadline)
                && !beatkernel_bms_runtime::player::cancelled()
            {
                input.poll(WallDuration::from_millis(1))?;
                check_group(&input, &options.local_players, &selected)?;
                if let Some(pair) = observe(&stream, &clock)? {
                    discipline.observe_clock_pair(pair)?;
                }
                feed_rendered(&mut bgm, stream.last_render_report(), |command| {
                    group.enqueue_audio(command)
                })?;
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
                if let Some(frontier) = merger.watermark(now, options.advance_lag, backlog)? {
                    while let Some(event) = merger.pop_ready(frontier)? {
                        let host = ClockPoint {
                            domain: event.meta().clock_domain,
                            timestamp: event.meta().timestamp,
                        };
                        discipline.validate_host(host)?;
                        match group.process_input(event, &ExplicitDomains, schedule(&stream)?) {
                            Ok(InputResult::Processed(reports)) => {
                                observe_reports(&reports, &mut states)?
                            }
                            Ok(InputResult::Ignored { device }) => {
                                return Err(format!(
                                    "merged source {device:?} has no local runtime owner"
                                )
                                .into());
                            }
                            Err(failure) => {
                                if let Err(error) =
                                    observe_reports(&failure.completed_reports, &mut states)
                                {
                                    eprintln!("partial group report observation: {error}");
                                }
                                return Err(failure.into());
                            }
                        }
                    }
                    match group.advance_to(frontier, &ExplicitDomains, schedule(&stream)?) {
                        Ok(reports) => observe_reports(&reports, &mut states)?,
                        Err(failure) => {
                            if let Err(error) =
                                observe_reports(&failure.completed_reports, &mut states)
                            {
                                eprintln!("partial group deadline observation: {error}");
                            }
                            return Err(failure.into());
                        }
                    }
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
