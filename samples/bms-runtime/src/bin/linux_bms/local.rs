//! Actual Linux local cohort: independent input/judging, one native audio owner.
use super::native::{ExplicitDomains, HOST, OUTPUT, observe, output_origin, schedule, seed};
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
    load_prepared,
    local_input::InputMerger,
    local_players::{MAX_LOCAL_PLAYERS, PlayerId},
    local_runtime::{InputResult, MemberConfig, PlayerReport, RuntimeGroup, VoiceAllocator},
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
    completion: SongCompletion,
    score: ScoreSummary,
    last_song: Timestamp,
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

pub(super) fn run(options: Options, competition_options: CompetitionOptions) -> Result<()> {
    admit_mode(
        options.local_inputs.len(),
        competition_options.network.is_some(),
    )?;
    let count = options.local_inputs.len();
    let clock = MonotonicClock::new(HOST);
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
            last_song: Timestamp::from_nanos(-options.preroll),
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
        prepared.bgm_commands,
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
                options.period as usize,
                capacity,
            )?,
        ),
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
            Timestamp::from_nanos(-options.preroll),
        )?;
        let pair = seed(&stream, &mut discipline, &mut bgm, &mut producer)?;
        let host_origin = ClockPoint {
            domain: HOST,
            timestamp: estimated_origin(pair, output_origin())?,
        };
        let mut group = RuntimeGroup::new(
            HOST,
            OUTPUT,
            Transport::new(
                host_origin.timestamp,
                Timestamp::from_nanos(-options.preroll),
                Rate::NORMAL,
            ),
            producer,
            configs,
            4096,
            &reserved,
        )?;
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
        let pump = (|| -> Result<()> {
            while deadline.is_none_or(|deadline| Instant::now() < deadline)
                && !beatkernel_bms_runtime::player::cancelled()
            {
                if let Some(pair) = observe(&stream, false)? {
                    discipline.observe_clock_pair(pair)?;
                }
                feed_rendered(&mut bgm, stream.last_render_report(), |command| {
                    group.enqueue_audio(command)
                })?;
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
                if let Some(frontier) = merger.watermark(
                    now,
                    options.advance_lag,
                    backlogged.iter().any(|backlog| *backlog),
                )? {
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
