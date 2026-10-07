//! Actual Windows local cohort: one Raw Input pump and one shared native output.
use super::native::{AcquisitionWindow, HOST, LOGICAL, OUTPUT};
use super::*;
#[cfg(test)]
use beatkernel::input::PhysicalControlId;
use beatkernel::{
    audio::PcmLimits,
    input::DeviceId,
    time::{ClockPoint, Timestamp},
    transport::Rate,
};
use beatkernel_bms_runtime::native_audio::{NativeAudioConfig, PreparedNativeAudio, prepare_audio};
use beatkernel_bms_runtime::native_cohort_setup::{
    activate_audio_cohort_with_sounds, admit_cohort as admit_mode, finish_cohort,
    finish_cohort_network, finish_cohort_with_results_and_network,
    prepare_audio_cohort_with_policy, CohortPreparation, PreparedCohort,
};
use beatkernel_bms_runtime::native_start::{
    MAX_START_INPUT_EVENTS, NativeStartConfig, start_committed,
};
use beatkernel_bms_runtime::{
    ChannelPolicy,
    competition_live::CompetitionOptions,
    local_players::PlayerId,
    native_chart::{NativeChartConfig, prepare_chart},
    playback_pause::NativePause,
};
#[cfg(test)]
use beatkernel_bms_runtime::{
    competition::ScoreSummary,
    local_input::InputMerger,
    native_cohort::{PlayerState, replay_path},
};
use beatkernel_bms_runtime::{
    native_cohort::{NativeAudioCohortSession, run_cohort_audio_with_policies_and_results},
    native_gameplay::{AudioGameplayConfig, NativeGameplayConfig},
};
#[cfg(test)]
use beatkernel_bms_runtime::{
    native_cohort::{finite_cohort_done, lag_reaches},
    playback_pause::{PauseKeyboard, PausePhase},
};
use beatkernel_platform::{
    audio::presentation::{discipline::DisciplineConfig, validation::NativePresentationValidator},
    raw_input::RawDeviceKind,
    windows::{clock::QpcClock, input::WindowsInput},
};

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
    beatkernel_bms_runtime::native_judge::validate_policy_competition(
        options.gauge,
        &competition_options,
    )?;
    admit_mode(
        options.local_players.len(),
        competition_options.network.is_some(),
    )?;
    let count = options.local_players.len();
    let song_origin = options.song_origin()?;
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
    let (prepared, section) = prepare_chart(NativeChartConfig {
        path: &options.chart,
        format: pcm,
        limits: PcmLimits::new(
            64 * 1024 * 1024,
            256 * 1024 * 1024,
            beatkernel_bms_runtime::DEFAULT_BMS_PCM_SAMPLES,
        )?,
        channels: if options.mono_stereo {
            ChannelPolicy::MonoToStereo
        } else {
            ChannelPolicy::Exact
        },
        chart_seed: options.chart_seed,
        start: Timestamp::from_nanos(options.start_ns),
        bindings: &options.bindings,
    })?;
    let policy = beatkernel_bms_runtime::native_judge::NativeJudgeConfig {
        early: options.early,
        late: options.late,
        offset: options.offset,
        preroll: options.preroll,
        output: OUTPUT,
        end: options.end_ns.map(Timestamp::from_nanos),
    }
    .resolve_play_policy(&section.original_gauge, options.gauge)?;
    println!("prepared practice section={section:?}");
    for warning in &prepared.source.warnings {
        eprintln!("BMS warning line{}: {}", warning.line, warning.message);
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
    let assignments: Vec<_> = options
        .local_players
        .iter()
        .enumerate()
        .map(|(index, &(player, _))| (player, selected[index].0))
        .collect();
    let PreparedCohort {
        mut network,
        configs,
        input_sounds,
        hazard_sounds,
        mut states,
        save_paths,
        reserved,
    } = prepare_audio_cohort_with_policy(
        &prepared,
        &assignments,
        &competition_options,
        &CohortPreparation {
            host: HOST,
            output: OUTPUT,
            early: options.early,
            late: options.late,
            offset: options.offset,
            preroll: options.preroll,
            start: Timestamp::from_nanos(options.start_ns),
            end: options.end_ns.map(Timestamp::from_nanos),
            chart_seed: options.chart_seed,
            bindings: &options.bindings,
            record_replay: options.record_replay.as_deref(),
            replay_max_bytes: options.replay_max_bytes,
            replay_max_records: options.replay_max_records,
        },
        LOGICAL,
        &policy,
    )?;
    let network_start = network.is_some();
    let PreparedNativeAudio {
        mut producer,
        bgm,
        mixer,
    } = match prepare_audio(
        prepared.bank,
        prepared.bgm_commands,
        NativeAudioConfig {
            output_origin,
            start: Timestamp::from_nanos(options.start_ns),
            preroll: Duration::from_nanos(options.preroll),
            lookahead: Duration::from_nanos(options.bgm_lookahead),
            voices: options.voices,
            max_render_frames: AudioLimits::MAX_RENDER_FRAMES,
            playback_end_frame: playback_end,
            gated_start: network_start,
        },
    ) {
        Ok(audio) => audio,
        Err(error) => {
            drop(setup);
            let mut failures = vec![format!("local audio preparation: {error}")];
            if let Err(close) = acquisition.registration.close() {
                failures.push(format!("input cleanup: {close}"));
            }
            finish_cohort_network(network.as_mut(), &states, &mut failures);
            return finish_cohort(states, save_paths, failures, save_capture);
        }
    };
    let mut bgm = BgmSession(bgm);
    let stream = match setup.open(mixer, &options, clock) {
        Ok(stream) => stream,
        Err(error) => {
            let mut failures = vec![format!("native output open: {error}")];
            if let Err(close) = acquisition.registration.close() {
                failures.push(format!("input cleanup: {close}"));
            }
            finish_cohort_network(network.as_mut(), &states, &mut failures);
            return finish_cohort(states, save_paths, failures, save_capture);
        }
    };
    let mut output = super::owned_output::owner(stream, clock);
    let mut output_ui = super::owned_output::WindowsOutputUi::new(&output, !network_start)?;
    println!(
        "local players={count}; shared output={}; independent judges/captures/scores",
        super::owned_output::stream(&mut output)?.description()
    );
    let mut before_origin = 0u64;
    let mut retained = std::collections::VecDeque::with_capacity(MAX_START_INPUT_EVENTS);
    let outcome = (|| -> Result<Option<Vec<(beatkernel_bms_runtime::local_players::PlayerId,beatkernel_bms_runtime::play_result::CompletedPlayResult)>>> {
        let start_basis = super::owned_output::basis(&output)?;
        let creation_epoch = output.last_issued_epoch();
        let committed_start = if let Some(network) =
            network.as_mut()
        {
            let started = {
                let mut device = super::native::StartupDevice {
                    stream: super::owned_output::stream(&mut output)?,
                    input: &mut input,
                    acquisition: &acquisition,
                    clock: &clock,
                    selected: &selected,
                    pre_origin: &mut before_origin,
                    retained: &mut retained,
                    physical: NativePresentationValidator::new(creation_epoch, start_basis.point_at_stream_frame(0)?, HOST),
                };
                start_committed(
                    &mut device,
                    network,
                    &mut producer,
                    &mut pause,
                    &mut native_end,
                    NativeStartConfig {
                        output_origin,
                        sample_rate: pcm.sample_rate(),
                        playback_end_frame: playback_end,
                        setup_timeout: competition_options.setup_timeout,
                        max_clock_age_ns: competition_options.start_policy.max_age_ns,
                        max_rate_error_ppm: DisciplineConfig::default().max_rate_error_ppm,
                    },
                    |report, producer| {
                        feed_rendered(&mut bgm, report, |command| producer.try_push(command))
                    },
                )?
            };
            let Some(started) = started else {
                return Ok(None);
            };
            let playback_origin = started.plan.selected_output();
            println!("shared native applied start={:?}; HOST acquisition={:?}; host window={:?}; physical accuracy unmeasured", started.plan, started.host_origin, started.host_window);
            Some((started.host_origin, playback_origin))
        } else {
            super::owned_output::stream(&mut output)?.start()?;
            None
        };
        let Some((mut presentation, seed, before)) = super::native::prime_output(&mut output, &mut input, &acquisition,
            &clock, &selected, &mut before_origin, &mut retained, &mut bgm, &mut producer, &mut native_end, committed_start.is_none())? else { return Ok(None); };
        let playback_origin = committed_start.map_or(output_origin, |(_, playback)| playback);
        let host_origin = match committed_start { Some((host, _)) => host, None => seed.host_for_output(playback_origin, before)? };
        let logical_origin = presentation.logical_output(playback_origin)?;
        let transport = beatkernel::transport::Transport::new(logical_origin.timestamp, song_origin, Rate::NORMAL);
        println!("shared audio-authoritative logical anchor={:?}; original associations={:?}; HOST acquisition={host_origin:?}; physical accuracy Unknown", transport.anchor(), seed.observations);
        let (mut group, mut merger) = activate_audio_cohort_with_sounds(
            configs,
            &reserved,
            host_origin,
            OUTPUT,
            logical_origin,
            transport,
            producer,
            options.end_ns.map(Timestamp::from_nanos),
            input_sounds,
            hazard_sounds,
        )?;
        let pump = {
            let mut device = super::native::GameplayDevice {
                output: &mut output,
                output_ui: &mut output_ui,
                input: &mut input,
                acquisition: &acquisition,
                clock: &clock,
                selected: &selected,
                retained: &mut retained,
            };
            let selected_policies: Vec<_> = assignments
                .iter()
                .map(|(player, _)| (*player, &policy))
                .collect();
            run_cohort_audio_with_policies_and_results(
                &mut device,
                NativeAudioCohortSession {
                    network: network.as_mut(),
                    group: &mut group,
                    states: &mut states,
                    merger: &mut merger,
                    bgm: &mut bgm,
                    discipline: &mut presentation,
                    pause: &mut pause,
                    end: &mut native_end,
                    delivery: &mut delivery,
                    pre_origin_inputs: &mut before_origin,
                },
                AudioGameplayConfig {
                    section_start: Timestamp::from_nanos(options.start_ns),
                    gameplay: NativeGameplayConfig {
                    origin: host_origin,
                    stream_origin: output_origin,
                    playback_origin,
                    song_origin,
                    sample_rate: pcm.sample_rate(),
                    end_song: options.end_ns.map(Timestamp::from_nanos),
                    advance_lag: Duration::from_nanos(options.advance_lag),
                    seconds: options.seconds,
                    pause_supported: !network_start,
                    logical_schedule: true,
                    },
                },
                &selected_policies,
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
    let stop = output.stop();
    let close = acquisition.registration.close();
    println!(
        "shared final output={:?}; pre-origin ignored={before_origin}; physical delivery unverified",
        output.current_mut().map(|out| out.native.description())
    );
    if let Err(error) = &stop {
        eprintln!("shared native stop/join error: {error}");
    }
    if let Err(error) = &close {
        eprintln!("Raw Input unregister error: {error}");
    }
    let mut failures = Vec::new();
    if let Err(error) = stop {
        failures.push(format!("output cleanup: {error}"));
    }
    if let Err(error) = close {
        failures.push(format!("input cleanup: {error}"));
    }
    finish_cohort_with_results_and_network(
        outcome,
        states,
        network.as_mut(),
        save_paths,
        failures,
        options.record_replay.as_deref(),
        save_capture,
        |archive, path| {
            beatkernel_bms_runtime::native_result_archive::save_cohort_sidecars(
                archive,
                path.ok_or("completed archive missing base replay path")?,
            )
        },
    )
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
                gauge: beatkernel_bms_runtime::gauge::BmsGauge::default(),
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
        assert!(admit_mode(2, true).is_ok());
        assert!(admit_mode(64, true).is_ok());
        for (count, network) in [(1, false), (65, false)] {
            assert!(admit_mode(count, network).is_err());
        }
    }
}
