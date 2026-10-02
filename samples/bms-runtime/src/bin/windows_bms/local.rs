//! Actual Windows local cohort: one Raw Input pump and one shared native output.
use super::native::{AcquisitionWindow, HOST, OUTPUT};
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
    load_prepared_with_seed,
    local_input::InputMerger,
    local_players::{MAX_LOCAL_PLAYERS, PlayerId},
    local_runtime::{MemberConfig, RuntimeGroup, VoiceAllocator},
    playback_pause::NativePause,
    replay_capture::LiveReplayCapture,
};
use beatkernel_bms_runtime::{
    native_cohort::{NativeCohortSession, PlayerState, replay_path, run_cohort},
    native_gameplay::NativeGameplayConfig,
};
#[cfg(test)]
use beatkernel_bms_runtime::{
    native_cohort::{finite_cohort_done, lag_reaches},
    playback_pause::{PauseKeyboard, PausePhase},
};
use beatkernel_platform::{
    audio::presentation::discipline::{DisciplineConfig, PresentationDiscipline},
    raw_input::RawDeviceKind,
    windows::{clock::QpcClock, input::WindowsInput},
};

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

pub(super) fn run(options: Options, competition_options: CompetitionOptions) -> Result<()> {
    admit_mode(
        options.local_players.len(),
        competition_options.network.is_some(),
    )?;
    let count = options.local_players.len();
    let song_origin = options.song_origin()?;
    let pause_supported = options.backend == Backend::Wasapi;
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
    let output_origin = ClockPoint {
        domain: OUTPUT,
        timestamp: Timestamp::ZERO,
    };
    let playback_end = options.playback_end(pcm.sample_rate())?;
    let mut pause = NativePause::new(output_origin, HOST, pcm.sample_rate())?;
    if let Some(end) = playback_end {
        pause = pause.with_playback_end_frame(end)?;
    }
    let mut native_end = playback_end
        .map(|end| {
            beatkernel_bms_runtime::native_end::NativeEnd::new(
                output_origin,
                HOST,
                pcm.sample_rate(),
                end,
            )
        })
        .transpose()?;
    let prepared = load_prepared_with_seed(
        &options.chart,
        pcm,
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
    beatkernel_bms_runtime::player::publish_native_chart(
        &options.chart,
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
        {
            let config = MixerConfig::new(
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
            );
            playback_end.map_or(config, |end| config.with_playback_end_frame(end))
        },
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
            song_origin,
        )?;
        let (mut transport, quality) = stream.calibrate(
            &options,
            calibration_extent(
                options.seconds.unwrap_or_else(|| {
                    states[0]
                        .completion
                        .as_ref()
                        .map_or(2, |c| c.calibration_seconds())
                }),
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
        if let Some(end) = options.end_ns {
            group.set_song_end(Timestamp::from_nanos(end))?;
        }
        let mut merger = InputMerger::new(
            HOST,
            host_origin,
            selected.iter().map(|(device, _)| *device).collect(),
            65536,
        )?;
        let pump = {
            let mut retained = std::collections::VecDeque::new();
            let mut device = super::native::GameplayDevice {
                stream: &mut stream,
                input: &mut input,
                acquisition: &acquisition,
                clock: &clock,
                selected: &selected,
                retained: &mut retained,
                last_snapshot: None,
            };
            run_cohort(
                &mut device,
                NativeCohortSession {
                    group: &mut group,
                    states: &mut states,
                    merger: &mut merger,
                    bgm: &mut bgm,
                    discipline: &mut discipline,
                    pause: &mut pause,
                    end: &mut native_end,
                    delivery: &mut delivery,
                    pre_origin_inputs: &mut before_origin,
                },
                NativeGameplayConfig {
                    origin: host_origin,
                    stream_origin: output_origin,
                    playback_origin: output_origin,
                    song_origin,
                    sample_rate: pcm.sample_rate(),
                    end_song: options.end_ns.map(Timestamp::from_nanos),
                    advance_lag: Duration::from_nanos(options.advance_lag),
                    seconds: options.seconds,
                    pause_supported,
                    logical_schedule: options.backend == Backend::Wasapi,
                },
            )
        };
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
use std::path::Path;
#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::input::ButtonState;
    fn host(ns: i64) -> ClockPoint {
        ClockPoint {
            domain: HOST,
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn button(
        device: u64,
        state: beatkernel::input::ButtonState,
        ns: i64,
        sequence: u64,
    ) -> beatkernel::input::PhysicalInputEvent {
        beatkernel::input::PhysicalInputEvent::Button(beatkernel::input::ButtonEvent {
            meta: beatkernel::input::EventMeta::new(DeviceId(device), host(ns), sequence),
            control: PhysicalControlId::keyboard(4),
            state,
        })
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
    fn pending_raw_receipts_preserve_merge_order_and_pause_key_provenance() {
        use beatkernel::{
            audio::{AudioCounters, RenderReport},
            input::{
                BackendId, ButtonEvent, ButtonState, EventMeta, NativeEventMeta, PhysicalInputEvent,
            },
            time::ClockPair,
        };
        let output = |ns| ClockPoint {
            domain: OUTPUT,
            timestamp: Timestamp::from_nanos(ns),
        };
        let pair = |ns| ClockPair {
            source: output(ns),
            target: host(ns + 100),
        };
        let button = |device, state, ns, sequence| {
            let mut meta = EventMeta::new(DeviceId(device), host(ns), sequence);
            meta.native = Some(NativeEventMeta {
                backend: BackendId(2),
                code: Some(4),
                timestamp: Some(host(ns)),
            });
            PhysicalInputEvent::Button(ButtonEvent {
                meta,
                control: PhysicalControlId::keyboard(4),
                state,
            })
        };
        let mut pause = NativePause::new(output(0), HOST, 1000).unwrap();
        let mut merger =
            InputMerger::new(HOST, host(100), vec![DeviceId(7), DeviceId(9)], 8).unwrap();
        let mut keys = PauseKeyboard::new();
        assert!(pause.request(true, pair(0)).unwrap());
        // Windows keeps collecting actual receipt points while awaiting output.
        let early = button(7, ButtonState::Down, 500_100, 1);
        let released = button(7, ButtonState::Up, 2_000_100, 2);
        merger.admit(early.clone(), host(500_101)).unwrap();
        merger
            .admit(button(9, ButtonState::Down, 1_500_100, 1), host(1_500_101))
            .unwrap();
        merger.admit(released.clone(), host(2_000_101)).unwrap();
        assert_eq!(pause.phase(), PausePhase::Pausing);
        let render = RenderReport {
            start_frame: 3,
            frames: 3,
            playback_start_frame: 1,
            playback_frames: 0,
            paused: true,
            playback_end_physical_frame: None,
            active_voices: 0,
            pending_commands: 0,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: AudioCounters::default(),
        };
        assert!(
            pause
                .observe(Some(render), pair(500_000))
                .unwrap()
                .is_none()
        );
        let boundary = pause.observe(None, pair(1_000_000)).unwrap().unwrap();
        assert_eq!(boundary.host, host(1_000_100));
        let exact = merger.pop_ready(boundary.host).unwrap().unwrap();
        assert_eq!(exact, early);
        assert!(keys.accept(&exact).unwrap());
        assert!(merger.pop_ready(boundary.host).unwrap().is_none());
        merger.commit(boundary.host).unwrap();
        while let Some(event) = merger.pop_ready(host(3_000_100)).unwrap() {
            keys.observe_paused(event).unwrap();
        }
        let releases = keys.resume(host(4_000_100)).unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].meta().source, DeviceId(7));
        assert_eq!(
            releases[0].meta().original_clock_point,
            Some(host(2_000_100))
        );
        assert_eq!(releases[0].meta().native, released.meta().native);
        assert!(
            !keys
                .accept(&button(9, ButtonState::Repeat, 4_000_101, 2))
                .unwrap()
        );
        assert!(
            !keys
                .accept(&button(9, ButtonState::Up, 4_000_102, 3))
                .unwrap()
        );
        assert!(
            keys.accept(&button(9, ButtonState::Down, 5_000_100, 4))
                .unwrap()
        );
    }
    #[test]
    fn immediate_pause_and_short_resume_wait_for_lag_without_masking_merger_regression() {
        let mut merger =
            InputMerger::new(HOST, host(0), vec![DeviceId(1), DeviceId(2)], 8).unwrap();
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
        assert!(
            merger
                .watermark(host(13_000_000), 2_000_000, true)
                .unwrap()
                .is_none()
        );
        assert!(!lag_reaches(host(i64::MIN), host(i64::MIN), 1).unwrap());
        assert!(lag_reaches(host(10), host(10), -1).is_err());
        assert!(
            lag_reaches(
                host(10),
                ClockPoint {
                    domain: OUTPUT,
                    timestamp: Timestamp::from_nanos(10)
                },
                0
            )
            .is_err()
        );
    }
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
