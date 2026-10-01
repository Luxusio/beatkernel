//! Actual Windows local cohort: one Raw Input pump and one shared native output.
use super::native::{AcquisitionWindow, ExplicitDomains, HOST, OUTPUT};
use super::*;
use beatkernel::{
    audio::{AudioCommand, Mixer, MixerConfig, PcmLimits, command_queue},
    input::{Binding, BindingMap, DeviceId, DeviceSelector, GameControlId, PhysicalControlId},
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    replay::codec::ReplayCodecLimits,
    time::{ClockPoint, Timestamp},
    transport::Rate,
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
    raw_input::RawDeviceKind,
    windows::{clock::QpcClock, input::WindowsInput},
};
use std::{
    path::Path,
    ptr,
    time::{Duration as WallDuration, Instant},
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DispatchMessageW, GIDC_ARRIVAL, GIDC_REMOVAL, MSG, PM_NOREMOVE, PM_REMOVE,
    PeekMessageW, TranslateMessage, WM_CLOSE, WM_INPUT, WM_INPUT_DEVICE_CHANGE, WM_QUIT,
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
        return Err("Windows local play requires 2..64 exact keyboard paths".into());
    }
    if network {
        return Err("network competition currently supports one local participant only".into());
    }
    Ok(())
}

/// Exact fresh attachment identities, without publishing native interface paths.
fn resolve_keyboards(
    requested: &[(PlayerId, String)],
    attached: &[(&str, u64, usize)],
) -> Result<Vec<(DeviceId, usize)>> {
    let mut ids = HashSet::new();
    let mut handles = HashSet::new();
    let mut players = HashSet::new();
    let mut selected = Vec::with_capacity(requested.len());
    for (player, path) in requested {
        if player.0 == 0 || !players.insert(*player) || path.is_empty() {
            return Err("invalid local player identity or keyboard path".into());
        }
        let (id, handle) = selected_keyboard(Some(path), attached.iter().copied())?
            .ok_or("local keyboard requires an exact attachment")?;
        if id == 0 || handle == 0 || !ids.insert(id) || !handles.insert(handle) {
            return Err("local keyboard paths alias the same native attachment".into());
        }
        selected.push((DeviceId(id), handle));
    }
    Ok(selected)
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
    if options.backend == Backend::Asio && !cfg!(feature = "asio-sdk") {
        return Err("ASIO requires feature asio-sdk and caller SDK; no driver loaded".into());
    }
    let clock = QpcClock::new(HOST)?;
    // Declared before native owners so delivery observations report after cleanup.
    let mut delivery = DeliverySession(beatkernel::telemetry::InputDeliveryTelemetry::new(
        4096, HOST,
    )?);
    // Declared before output setup so exceptional exits close output owners first.
    let mut acquisition = AcquisitionWindow::new()?;
    let mut input = WindowsInput::new(clock);
    let devices = input.enumerate_devices()?;
    let attached: Vec<_> = devices
        .iter()
        .filter(|device| device.kind == RawDeviceKind::Keyboard)
        .map(|device| {
            (
                device.interface_path.as_str(),
                device.descriptor.runtime_id.0,
                device.handle,
            )
        })
        .collect();
    let selected = resolve_keyboards(&options.local_players, &attached)?;
    let setup = super::live_output::Setup::new(&options, clock)?;
    let pcm = setup.format();
    let prepared = load_prepared(
        &options.chart,
        pcm,
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
        let device = selected[index].0;
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
            output_origin: ClockPoint {
                domain: OUTPUT,
                timestamp: Timestamp::ZERO,
            },
            sample_rate: pcm.sample_rate(),
            preroll: Duration::from_nanos(options.preroll),
            lookahead: Duration::from_nanos(options.bgm_lookahead),
            max_pending: capacity - SLACK,
        },
    )?);
    bgm.feed(0, capacity - SLACK, |command| producer.try_push(command))?;
    let mixer = Mixer::new(
        MixerConfig::new(
            pcm,
            OUTPUT,
            Timestamp::ZERO,
            AudioLimits::new(
                capacity,
                options.voices,
                capacity,
                AudioLimits::MAX_RENDER_FRAMES,
                capacity,
            )?,
        ),
        prepared.bank,
        consumer,
    )?;
    let mut stream = setup.open(mixer, &options, clock)?;
    println!(
        "local players={count}; shared output={}; independent judges/captures/scores",
        stream.description()
    );
    let mut before_origin = 0u64;
    let outcome = (|| -> Result<()> {
        stream.start()?;
        let mut discipline = PresentationDiscipline::new(
            DisciplineConfig::default(),
            ClockPoint {
                domain: OUTPUT,
                timestamp: Timestamp::ZERO,
            },
            HOST,
            options.song_origin()?,
        )?;
        let (mut transport, quality) = stream.calibrate(
            &options,
            calibration_extent(
                options
                    .seconds
                    .unwrap_or(states[0].completion.calibration_seconds()),
                options.preroll,
            )?,
            &mut bgm,
            &mut producer,
        )?;
        transport.set_rate(transport.anchor().host_time, Rate::NORMAL)?;
        stream.seed(&mut discipline, &mut bgm, &mut producer)?;
        let host_origin = ClockPoint {
            domain: HOST,
            timestamp: transport.anchor().host_time,
        };
        println!(
            "shared output origin={host_origin:?}; mapping quality={quality:?}; physical latency unmeasured"
        );
        let mut group =
            RuntimeGroup::new(HOST, OUTPUT, transport, producer, configs, 4096, &reserved)?;
        let mut merger = InputMerger::new(
            HOST,
            host_origin,
            selected.iter().map(|(device, _)| *device).collect(),
            65536,
        )?;
        let deadline = options
            .seconds
            .map(|seconds| Instant::now() + WallDuration::from_secs(seconds));
        let mut last_progress = None;
        let pump = (|| -> Result<()> {
            'pump: while deadline.is_none_or(|deadline| Instant::now() < deadline)
                && !beatkernel_bms_runtime::player::cancelled()
            {
                let _admission = stream.observe(&mut discipline)?;
                feed_rendered(&mut bgm, stream.render_report()?, |command| {
                    group.enqueue_audio(command)
                })?;
                // Drain a bounded common pump before releasing input or miss deadlines.
                // SAFETY: MSG is initialized POD storage on its owning window thread.
                let mut message: MSG = unsafe { std::mem::zeroed() };
                for _ in 0..256 {
                    if deadline.is_some_and(|deadline| Instant::now() >= deadline)
                        || beatkernel_bms_runtime::player::cancelled()
                    {
                        break 'pump;
                    }
                    // SAFETY: writable message storage, common owning-thread queue.
                    if unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } == 0
                    {
                        break;
                    }
                    if message.message == WM_QUIT || message.message == WM_CLOSE {
                        break 'pump;
                    }
                    if message.hwnd == acquisition.hwnd() && message.message == WM_INPUT {
                        let acquired =
                            input.read_raw_input(message.lParam as usize, Some(message.time));
                        if message.wParam & 0xff == 0 {
                            // SAFETY: clean this actual foreground Raw Input message once,
                            // including when decode failed. Never dispatch it again below.
                            unsafe {
                                DefWindowProcW(
                                    message.hwnd,
                                    message.message,
                                    message.wParam,
                                    message.lParam,
                                );
                            }
                        }
                        for event in acquired?.input.events {
                            if !selected
                                .iter()
                                .any(|(device, _)| *device == event.meta().source)
                            {
                                continue;
                            }
                            let host = ClockPoint {
                                domain: event.meta().clock_domain,
                                timestamp: event.meta().timestamp,
                            };
                            let received = clock.sample()?.normalized;
                            discipline.validate_host(received)?;
                            if host.domain != HOST || host.timestamp > received.timestamp {
                                return Err(
                                    "local Raw Input has invalid QPC domain/future timestamp"
                                        .into(),
                                );
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
                        continue;
                    }
                    if message.hwnd == acquisition.hwnd()
                        && message.message == WM_INPUT_DEVICE_CHANGE
                    {
                        match message.wParam as u32 {
                            GIDC_ARRIVAL => {
                                input.attach_device(message.lParam as usize)?;
                            }
                            GIDC_REMOVAL => {
                                if selected
                                    .iter()
                                    .any(|(_, handle)| *handle == message.lParam as usize)
                                {
                                    return Err(
                                        "assigned local keyboard detached; restart whole cohort"
                                            .into(),
                                    );
                                }
                                input.remove_device(message.lParam as usize);
                            }
                            _ => {}
                        }
                    }
                    // SAFETY: an actual initialized native message on the owning thread.
                    unsafe {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                // SAFETY: inspect without removing; any queued input/owner message
                // freezes the common frontier until a subsequent bounded pump drains it.
                let backlog =
                    unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_NOREMOVE) } != 0;
                let now = clock.sample()?.normalized;
                discipline.validate_host(now)?;
                // A calibrated output-zero host anchor can still be in the
                // future. Keep that anchor intact until its actual host origin.
                if now.timestamp < host_origin.timestamp {
                    std::thread::sleep(WallDuration::from_millis(1));
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
                        match group.process_input(
                            event,
                            &ExplicitDomains,
                            stream.schedule(pcm.sample_rate())?,
                        ) {
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
                    match group.advance_to(
                        frontier,
                        &ExplicitDomains,
                        stream.schedule(pcm.sample_rate())?,
                    ) {
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
                        stream.render_report()?,
                        discipline.latest_pair().map(|pair| pair.source),
                    )?;
                }
                if finished {
                    println!(
                        "all Windows local players complete: terminal judging and shared output drain"
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
    // Stop/join every output callback before unregistering physical acquisition,
    // finishing ghosts, and attempting every independent replay save.
    let stop = stream.stop();
    let close = acquisition.registration.close();
    println!(
        "shared final output={}; pre-origin ignored={before_origin}; physical delivery unverified",
        stream.description()
    );
    if let Err(error) = &stop {
        eprintln!("shared native stop/join error: {error}");
    }
    if let Err(error) = &close {
        eprintln!("Raw Input unregister error: {error}");
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
    fn exact_four_keyboard_resolution_preserves_native_ids_and_rejects_aliases() {
        let requested = vec![
            (PlayerId(7), "path:a".into()),
            (PlayerId(1000), "path:b".into()),
            (PlayerId(u32::MAX), "path:c".into()),
            (PlayerId(3), "path:d".into()),
        ];
        let attached = [
            ("path:a", 11, 111),
            ("path:b", 22, 222),
            ("path:c", 33, 333),
            ("path:d", 44, 444),
        ];
        assert_eq!(
            resolve_keyboards(&requested, &attached).unwrap(),
            vec![
                (DeviceId(11), 111),
                (DeviceId(22), 222),
                (DeviceId(33), 333),
                (DeviceId(44), 444)
            ]
        );
        assert!(resolve_keyboards(&requested, &attached[..3]).is_err());
        let mut ambiguous = attached.to_vec();
        ambiguous.push(("path:a", 55, 555));
        assert!(resolve_keyboards(&requested, &ambiguous).is_err());
        for alias in [("path:d", 11, 444), ("path:d", 44, 111)] {
            let mut invalid = attached;
            invalid[3] = alias;
            assert!(resolve_keyboards(&requested, &invalid).is_err());
        }
        let mut duplicate_player = requested.clone();
        duplicate_player[3].0 = PlayerId(7);
        assert!(resolve_keyboards(&duplicate_player, &attached).is_err());
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
