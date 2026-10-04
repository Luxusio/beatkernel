//! Actual Linux local cohort: independent input/judging, one native audio owner.
use super::native::{HOST, OUTPUT, observe, output_origin, seed, startup_input};
use super::*;
#[cfg(test)]
use beatkernel::input::PhysicalControlId;
use beatkernel::{
    audio::PcmLimits,
    input::DeviceId,
    time::Duration,
    transport::{Rate, Transport},
};
use beatkernel_bms_runtime::native_audio::{NativeAudioConfig, PreparedNativeAudio, prepare_audio};
use beatkernel_bms_runtime::native_cohort_setup::{
    CohortPreparation, PreparedCohort, activate_cohort, admit_cohort as admit_mode, finish_cohort,
    prepare_cohort,
};
use beatkernel_bms_runtime::{
    ChannelPolicy,
    competition_live::CompetitionOptions,
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
#[cfg(test)]
use beatkernel_bms_runtime::{
    local_players::PlayerId,
    native_cohort::{finite_cohort_done, lag_reaches},
    playback_pause::PauseKeyboard,
};
use beatkernel_bms_runtime::{
    native_cohort::{NativeCohortSession, run_cohort},
    native_gameplay::{
        InputBatch, NativeGameplayConfig, NativeGameplayDevice, NativeGameplayResult, retain_input,
    },
};
use beatkernel_platform::{
    audio::{
        DeviceFormat, SampleEncoding,
        presentation::discipline::{DisciplineConfig, PresentationDiscipline},
    },
    linux::{AlsaRequest, AlsaStream, EvdevDevice, EvdevItem, MonotonicClock},
};

use beatkernel_bms_runtime::native_start::{
    NativeStartConfig, NativeStartDevice, NativeStartObservation, NativeStartResult,
    start_committed,
};
use std::collections::VecDeque;
struct CohortStartupDevice<'a> {
    stream: &'a mut AlsaStream,
    inputs: &'a mut [EvdevDevice],
    clock: &'a MonotonicClock,
    before_origin: &'a mut u64,
    retained: &'a mut VecDeque<beatkernel::input::PhysicalInputEvent>,
    observed: bool,
}
impl NativeStartDevice for CohortStartupDevice<'_> {
    type Evidence = ();
    fn start(&mut self) -> NativeStartResult<()> {
        Ok(self.stream.start()?)
    }
    fn service_input(&mut self, retain: bool) -> NativeStartResult<bool> {
        for input in &mut *self.inputs {
            if !startup_input(
                input,
                self.before_origin,
                if retain {
                    Some(&mut *self.retained)
                } else {
                    None
                },
            )? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn observe(&mut self) -> NativeStartResult<Option<NativeStartObservation<()>>> {
        let pair = observe(self.stream, !self.observed)?;
        self.observed |= pair.is_some();
        Ok(pair.map(|pair| NativeStartObservation {
            timing: pair.into(),
            evidence: (),
        }))
    }
    fn render_report(&mut self) -> NativeStartResult<Option<beatkernel::audio::RenderReport>> {
        Ok(self.stream.last_render_report())
    }
    fn buffer_frames(&self) -> NativeStartResult<u32> {
        Ok(self.stream.configuration().buffer_frames)
    }
    fn host_now(&self) -> NativeStartResult<ClockPoint> {
        Ok(self.clock.now()?)
    }
}

struct CohortDevice<'a> {
    stream: &'a mut AlsaStream,
    inputs: &'a mut [EvdevDevice],
    clock: &'a MonotonicClock,
    backlogged: &'a mut [bool],
    retained: &'a mut VecDeque<beatkernel::input::PhysicalInputEvent>,
}
impl NativeGameplayDevice for CohortDevice<'_> {
    fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
        if let Some(pair) = observe(self.stream, false)? {
            discipline.observe_clock_pair(pair)?;
        }
        Ok(())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<beatkernel::audio::RenderReport>> {
        Ok(self.stream.last_render_report())
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(self.clock.now()?)
    }
    fn acquire(
        &mut self,
        events: &mut std::collections::VecDeque<beatkernel::input::PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        if self.inputs.len() != self.backlogged.len() || self.inputs.is_empty() {
            return Err("invalid local evdev ownership".into());
        }
        for _ in 0..256 {
            let Some(event) = self.retained.pop_front() else {
                break;
            };
            retain_input(events, event)?;
        }
        if !self.retained.is_empty() {
            return Ok(InputBatch {
                backlog: true,
                closed: false,
            });
        }
        self.backlogged.fill(true);
        for _ in 0..256 {
            if self.backlogged.iter().all(|pending| !pending) {
                break;
            }
            for (index, input) in self.inputs.iter_mut().enumerate() {
                if !self.backlogged[index] {
                    continue;
                }
                match input.read_next()? {
                    EvdevItem::WouldBlock => self.backlogged[index] = false,
                    EvdevItem::Ignored => {}
                    EvdevItem::Event(event) => retain_input(events, event)?,
                    EvdevItem::Dropped | EvdevItem::Resync(_) => {
                        return Err("local evdev loss/resync; restart whole cohort".into());
                    }
                }
            }
        }
        Ok(InputBatch {
            backlog: self.backlogged.iter().any(|pending| *pending),
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
        Err("ALSA local cohorts use logical mixer scheduling".into())
    }
}

pub(super) fn run(options: Options, competition_options: CompetitionOptions) -> Result<()> {
    admit_mode(options.local_inputs.len(), false)?;
    let playback_end = options.playback_end()?;
    let count = options.local_inputs.len();
    if options.local_players.len() != count {
        return Err("local player and native input assignment counts differ".into());
    }
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
        &options.local_players,
    )?;
    if beatkernel_bms_runtime::player::cancelled() {
        return Ok(());
    }
    let assignments = options
        .local_players
        .iter()
        .enumerate()
        .map(|(index, &player)| Ok((player, DeviceId(u64::try_from(index + 1)?))))
        .collect::<Result<Vec<_>>>()?;
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
    let PreparedNativeAudio {
        mut producer,
        bgm,
        mixer,
    } = prepare_audio(
        prepared.bank,
        prepared.bgm_commands,
        NativeAudioConfig {
            output_origin: output_origin(),
            start: Timestamp::from_nanos(options.start_ns),
            preroll: Duration::from_nanos(options.preroll),
            lookahead: Duration::from_nanos(options.bgm_lookahead),
            voices: options.voices,
            max_render_frames: options.period as usize,
            playback_end_frame: playback_end,
            gated_start: network.is_some(),
        },
    )?;
    let mut bgm = BgmSession(bgm);
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
    let mut startup_inputs = VecDeque::new();
    let outcome = (|| -> Result<()> {
        let (mut discipline, host_origin, playback_origin) = if let Some(network) = network.as_mut()
        {
            let started = start_committed(
                &mut CohortStartupDevice {
                    stream: &mut stream,
                    inputs: &mut inputs,
                    clock: &clock,
                    before_origin: &mut before_origin,
                    retained: &mut startup_inputs,
                    observed: false,
                },
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
                    beatkernel_bms_runtime::native_audio::feed_rendered(
                        &mut bgm,
                        report,
                        |command| producer.try_push(command),
                    )
                },
            )?;
            let Some(started) = started else {
                return Ok(());
            };
            let selected = started.plan.selected_output();
            let mut discipline = PresentationDiscipline::new_with_playback_origin(
                DisciplineConfig::default(),
                output_origin(),
                selected,
                HOST,
                song_origin,
            )?;
            discipline.observe_clock_pair(started.observation.timing.point()?)?;
            (discipline, started.host_origin, selected)
        } else {
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
            (discipline, host_origin, output_origin())
        };
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
            let mut backlogged = vec![true; count];
            let mut device = CohortDevice {
                stream: &mut stream,
                inputs: &mut inputs,
                clock: &clock,
                backlogged: &mut backlogged,
                retained: &mut startup_inputs,
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
                    pause_supported: competition_options.network.is_none(),
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
    let mut failures = Vec::new();
    if let Err(error) = outcome {
        failures.push(format!("local session: {error}"));
    }
    if let Err(error) = stop {
        failures.push(format!("output cleanup: {error}"));
    }
    beatkernel_bms_runtime::native_cohort_setup::finish_cohort_network(
        network.as_mut(),
        &states,
        &mut failures,
    );
    finish_cohort(states, save_paths, failures, save_capture)
}

#[cfg(test)]
use std::path::Path;
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
    fn local_roster_bounds_are_shared_by_offline_and_network_modes() {
        assert!(admit_mode(2, false).is_ok());
        assert!(admit_mode(64, false).is_ok());
        assert!(admit_mode(2, true).is_ok());
        assert!(admit_mode(64, true).is_ok());
        for (count, network) in [(1, false), (65, false), (1, true), (65, true)] {
            assert!(admit_mode(count, network).is_err());
        }
    }
}
