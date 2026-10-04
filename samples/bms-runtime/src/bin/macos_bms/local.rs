//! Actual macOS local cohort: one IOHID owner, one shared CoreAudio output.
use super::native::{HOST, M_NATIVE, OUTPUT, observe, output_origin};
use super::*;
#[cfg(test)]
use beatkernel::input::PhysicalControlId;
use beatkernel::{
    audio::{CommandProducer, PcmLimits},
    input::DeviceId,
    time::Duration,
    transport::{Rate, Transport},
};
use beatkernel_bms_runtime::native_audio::{NativeAudioConfig, PreparedNativeAudio, prepare_audio};
use beatkernel_bms_runtime::native_cohort_setup::{
    CohortPreparation, PreparedCohort, activate_cohort, admit_cohort as admit_mode, finish_cohort,
    finish_cohort_network, prepare_cohort,
};
use beatkernel_bms_runtime::native_start::{MAX_START_INPUT_EVENTS, NativeStartConfig, start_committed};
use beatkernel_bms_runtime::{
    ChannelPolicy,
    competition_live::CompetitionOptions,
    local_players::PlayerId,
    native_chart::{NativeChartConfig, prepare_chart},
    native_end::NativeEnd,
    playback_pause::NativePause,
};
#[cfg(test)]
use beatkernel_bms_runtime::{
    competition::ScoreSummary,
    local_input::InputMerger,
    native_cohort::{PlayerState, replay_path},
};
use beatkernel_bms_runtime::{
    native_cohort::{NativeCohortSession, run_cohort},
    native_gameplay::{
        InputBatch, NativeGameplayConfig, NativeGameplayDevice, NativeGameplayResult, retain_input,
    },
};
#[cfg(test)]
use beatkernel_bms_runtime::{
    native_cohort::{finite_cohort_done, lag_reaches},
    playback_pause::PauseKeyboard,
};
use beatkernel_platform::{
    audio::presentation::discipline::{DisciplineConfig, PresentationDiscipline},
    macos::{
        audio::{CoreAudioRequest, CoreAudioStream},
        clock::MachClock,
        input::{HidCounters, HidInput, HidSample},
    },
};
use std::time::{Duration as WallDuration, Instant};

struct CohortDevice<'a> {
    stream: &'a mut CoreAudioStream,
    input: &'a mut HidInput,
    clock: &'a MachClock,
    selected: &'a [DeviceId],
    assignments: &'a [(PlayerId, u64)],
    retained: &'a mut std::collections::VecDeque<HidSample>,
}
impl NativeGameplayDevice for CohortDevice<'_> {
    fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
        self.input.poll(WallDuration::from_millis(1))?;
        check_group(self.input, self.assignments, self.selected)?;
        if let Some(pair) = observe(self.stream, self.clock)? {
            discipline.observe_clock_pair(pair)?;
        }
        Ok(())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<beatkernel::audio::RenderReport>> {
        Ok(self.stream.last_render_report())
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(self.clock.sample()?.normalized)
    }
    fn acquire(
        &mut self,
        events: &mut std::collections::VecDeque<beatkernel::input::PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        for _ in 0..256 {
            let sample = self.retained.pop_front().or_else(|| self.input.pop());
            let Some(sample) = sample else {
                return Ok(InputBatch {
                    backlog: false,
                    closed: false,
                });
            };
            if self.selected.contains(&sample.event.meta().source) {
                retain_input(events, sample.event)?;
            }
        }
        Ok(InputBatch {
            backlog: true,
            closed: false,
        })
    }
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        discipline: &PresentationDiscipline,
        report: Option<beatkernel::audio::RenderReport>,
    ) -> NativeGameplayResult<Option<beatkernel_bms_runtime::native_end::EndBoundary>> {
        Ok(end.observe(
            report,
            discipline
                .latest_pair()
                .ok_or("native end clock relation missing")?,
        )?)
    }
    fn seed_resume(
        &mut self,
        discipline: &mut PresentationDiscipline,
        reference: ClockPair,
    ) -> NativeGameplayResult<()> {
        discipline.observe_clock_pair(reference)?;
        Ok(())
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        Err("CoreAudio local cohorts use logical mixer scheduling".into())
    }
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

pub(super) fn run(options: Options, competition_options: CompetitionOptions) -> Result<()> {
    admit_mode(
        options.local_players.len(),
        competition_options.network.is_some(),
    )?;
    let playback_end = options.playback_end()?;
    let count = options.local_players.len();
    let song_origin = options.song_origin()?;
    // Check the actual output grid before assets or native owners are acquired.
    let mut pause = NativePause::new(output_origin(), HOST, options.format.sample_rate())?;
    if let Some(end) = playback_end {
        pause = pause.with_playback_end_frame(end)?;
    }
    let mut native_end = playback_end
        .map(|end| NativeEnd::new(output_origin(), HOST, options.format.sample_rate(), end))
        .transpose()?;
    let clock = MachClock::new(M_NATIVE, HOST)?;
    // Declared before native owners so delivery observations report after cleanup.
    let mut delivery = DeliverySession(beatkernel::telemetry::InputDeliveryTelemetry::new(
        4096, HOST,
    )?);
    let (prepared, section) = prepare_chart(NativeChartConfig {
        path: &options.chart,
        format: options.format,
        limits: PcmLimits::new(64 * 1024 * 1024, 256 * 1024 * 1024, 1295)?,
        channels: if options.mono_stereo {
            ChannelPolicy::MonoToStereo
        } else {
            ChannelPolicy::Exact
        },
        chart_seed: options.chart_seed,
        start: Timestamp::from_nanos(options.start_ns),
        bindings: &options.bindings,
    })?;
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
    let assignments: Vec<_> = options
        .local_players
        .iter()
        .enumerate()
        .map(|(index, &(player, _))| (player, selected[index]))
        .collect();
    let PreparedCohort {
        mut network,
        configs,
        mut states,
        save_paths,
        reserved,
    } = prepare_cohort(
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
            output_origin: output_origin(),
            start: Timestamp::from_nanos(options.start_ns),
            preroll: Duration::from_nanos(options.preroll),
            lookahead: Duration::from_nanos(options.bgm_lookahead),
            voices: options.voices,
            max_render_frames: options.buffer as usize,
            playback_end_frame: playback_end,
            gated_start: network_start,
        },
    ) {
        Ok(audio) => audio,
        Err(error) => {
            let mut failures = vec![format!("local audio preparation: {error}")];
            if let Err(close) = input.close() {
                failures.push(format!(
                    "IOHID close after audio preparation failure: {close}"
                ));
            }
            finish_cohort_network(network.as_mut(), &states, &mut failures);
            return finish_cohort(states, save_paths, failures, save_capture);
        }
    };
    let mut bgm = BgmSession(bgm);
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
            finish_cohort_network(network.as_mut(), &states, &mut failures);
            return finish_cohort(states, save_paths, failures, save_capture);
        }
    };
    println!(
        "macOS local players={count}; shared requested/applied CoreAudio={:?}; independent judges/captures/scores",
        stream.configuration()
    );
    let mut before_origin = 0u64;
    let mut other_devices = 0u64;
    let mut retained = std::collections::VecDeque::with_capacity(MAX_START_INPUT_EVENTS);
    let outcome = (|| -> Result<()> {
        check_group(&input, &options.local_players, &selected)?;
        let (mut discipline, host_origin, playback_origin) = if let Some(network) = network.as_mut()
        {
            let started = {
                let check_selection =
                    |input: &HidInput| check_group(input, &options.local_players, &selected);
                let mut device = super::native::StartupDevice {
                    audio: &mut stream,
                    input: &mut input,
                    clock: &clock,
                    selected: &selected,
                    check_selection: &check_selection,
                    pre_origin: &mut before_origin,
                    other_devices: &mut other_devices,
                    retained: &mut retained,
                };
                start_committed(
                    &mut device,
                    network,
                    &mut producer,
                    &mut pause,
                    &mut native_end,
                    NativeStartConfig {
                        output_origin: output_origin(),
                        sample_rate: options.format.sample_rate(),
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
                return Ok(());
            };
            let playback_origin = started.plan.selected_output();
            let mut discipline = PresentationDiscipline::new_with_playback_origin(
                DisciplineConfig::default(),
                output_origin(),
                playback_origin,
                HOST,
                song_origin,
            )?;
            discipline.observe_clock_pair(started.observation.timing.point()?)?;
            println!(
                "shared native applied start={:?}; host={:?}; physical accuracy unmeasured",
                started.plan, started.host_origin
            );
            (discipline, started.host_origin, playback_origin)
        } else {
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
            (discipline, host_origin, output_origin())
        };
        discipline.validate_host(clock.sample()?.normalized)?;
        let (mut group, mut merger) = activate_cohort(
            configs,
            &reserved,
            host_origin,
            OUTPUT,
            Transport::new(host_origin.timestamp, song_origin, Rate::NORMAL),
            producer,
            options.end_ns.map(Timestamp::from_nanos),
        )?;
        let pump = {
            let mut device = CohortDevice {
                stream: &mut stream,
                input: &mut input,
                clock: &clock,
                selected: &selected,
                assignments: &options.local_players,
                retained: &mut retained,
            };
            run_cohort(
                &mut device,
                NativeCohortSession {
                    network: network.as_mut(),
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
                    stream_origin: output_origin(),
                    playback_origin,
                    song_origin,
                    sample_rate: options.format.sample_rate(),
                    end_song: options.end_ns.map(Timestamp::from_nanos),
                    advance_lag: Duration::from_nanos(options.advance_lag),
                    seconds: options.seconds,
                    pause_supported: !network_start,
                    logical_schedule: true,
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
    // Stop native audio callbacks before unregistering HID and writing files.
    let stop = stream.stop();
    let close = input.close();
    println!(
        "shared final CoreAudio={:?}; HID counters={:?}; pre-origin ignored={before_origin}; other-device startup inputs={other_devices}; physical delivery unverified",
        stream.snapshot(),
        input.counters()
    );
    if let Err(error) = &stop {
        eprintln!("CoreAudio stop error: {error}");
    }
    if let Err(error) = &close {
        eprintln!("IOHID close error: {error}");
    }
    let mut failures = Vec::new();
    if let Err(error) = outcome {
        failures.push(format!("local session: {error}"));
    }
    if let Err(error) = stop {
        failures.push(format!("output cleanup: {error}"));
    }
    if let Err(error) = close {
        failures.push(format!("input cleanup: {error}"));
    }
    finish_cohort_network(network.as_mut(), &states, &mut failures);
    finish_cohort(states, save_paths, failures, save_capture)
}

#[cfg(test)]
use std::path::Path;
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
    fn finite_cohort_requires_all_members_native_ack_empty_collector_and_actual_commit() {
        for count in [2, 3, 4, 64] {
            let mut ids: Vec<_> = (0..count).map(|i| PlayerId(1000 + i as u32 * 7)).collect();
            *ids.last_mut().unwrap() = PlayerId(u32::MAX);
            let mut states = finite_states(&ids, 50);
            assert!(finite_cohort_done(
                Some(50),
                Some(host(20)),
                Some(host(20)),
                &states,
                false,
                false
            ));
            for (presented, committed, backlog, resuming) in [
                (None, Some(host(20)), false, false),
                (Some(host(20)), None, false, false),
                (Some(host(20)), Some(host(19)), false, false),
                (Some(host(20)), Some(host(20)), true, false),
                (Some(host(20)), Some(host(20)), false, true),
            ] {
                assert!(!finite_cohort_done(
                    Some(50),
                    presented,
                    committed,
                    &states,
                    backlog,
                    resuming
                ));
            }
            states.last_mut().unwrap().last_song = Timestamp::from_nanos(49);
            assert!(!finite_cohort_done(
                Some(50),
                Some(host(20)),
                Some(host(21)),
                &states,
                false,
                false
            ));
            states.last_mut().unwrap().last_song = Timestamp::from_nanos(50);
            let wrong = ClockPoint {
                domain: OUTPUT,
                timestamp: Timestamp::from_nanos(20),
            };
            assert!(!finite_cohort_done(
                Some(50),
                Some(wrong),
                Some(host(21)),
                &states,
                false,
                false
            ));
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
    fn finite_global_prefix_reconciles_release_before_exclusive_terminal_and_commits_only_after_pop()
     {
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
        let selected = resolve_registries(&requested, &attached).unwrap().unwrap();
        let states = finite_states(
            &requested
                .iter()
                .map(|(player, _)| *player)
                .collect::<Vec<_>>(),
            50,
        );
        let mut merger = InputMerger::new(HOST, host(0), selected, 16).unwrap();
        let mut keyboard = PauseKeyboard::new();
        keyboard
            .accept(&paused_button(DeviceId(11), 4, ButtonState::Down, 1, 1))
            .unwrap();
        merger.commit(host(10)).unwrap();
        let release = paused_button(DeviceId(11), 4, ButtonState::Up, 19, 2);
        let earlier = paused_button(DeviceId(33), 4, ButtonState::Down, 21, 1);
        for event in [
            release.clone(),
            earlier.clone(),
            paused_button(DeviceId(22), 4, ButtonState::Down, 22, 1),
            paused_button(DeviceId(44), 4, ButtonState::Down, 23, 1),
            paused_button(DeviceId(22), 4, ButtonState::Up, 31, 2),
        ] {
            merger.admit(event, host(40)).unwrap();
        }
        assert_eq!(merger.watermark(host(30), 2, true).unwrap(), None);
        let candidate = merger.watermark(host(30), 2, false).unwrap().unwrap();
        assert!(merger.commit(candidate).is_err());
        assert!(!finite_cohort_done(
            Some(50),
            Some(host(22)),
            Some(host(10)),
            &states,
            false,
            false
        ));
        let mut resume = Some(host(20));
        let mut gameplay = Vec::new();
        let mut acquired = Vec::new();
        while let Some(event) = merger.pop_ready(candidate).unwrap() {
            let at = ClockPoint {
                domain: event.meta().clock_domain,
                timestamp: event.meta().timestamp,
            };
            acquired.push(event.meta().source);
            if let Some(boundary) = resume {
                if at.timestamp < boundary.timestamp {
                    keyboard.observe_paused(event).unwrap();
                    continue;
                }
                gameplay.extend(keyboard.resume(boundary).unwrap());
                resume = None;
            }
            if !before_finite_end(at, Some(host(22))).unwrap() {
                continue;
            }
            if keyboard.accept(&event).unwrap() {
                gameplay.push(event);
            }
        }
        assert_eq!(
            acquired,
            vec![DeviceId(11), DeviceId(33), DeviceId(22), DeviceId(44)]
        );
        assert!(resume.is_none());
        assert_eq!(gameplay.len(), 2);
        assert_eq!(gameplay[0].meta().source, DeviceId(11));
        assert_eq!(gameplay[0].meta().timestamp, host(20).timestamp);
        assert_eq!(gameplay[0].meta().original_clock_point, Some(host(19)));
        assert_eq!(gameplay[0].meta().native, release.meta().native);
        assert_eq!(gameplay[1], earlier);
        merger.commit(candidate).unwrap();
        assert!(finite_cohort_done(
            Some(50),
            Some(host(22)),
            Some(candidate),
            &states,
            false,
            false
        ));
        assert_eq!(merger.pending(), 1); // Future post-end input survives until owner cleanup.
        assert!(
            merger
                .admit(
                    paused_button(DeviceId(44), 4, ButtonState::Up, 27, 2),
                    host(40)
                )
                .is_err()
        );
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
        assert!(admit_mode(2, true).is_ok());
        assert!(admit_mode(64, true).is_ok());
        for (count, network) in [(1, false), (65, false)] {
            assert!(admit_mode(count, network).is_err());
        }
    }
}
