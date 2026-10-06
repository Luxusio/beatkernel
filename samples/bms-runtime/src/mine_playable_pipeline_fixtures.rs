//! Deferred mine owners beginning at the actual injected preparation boundary.
use crate::{
    AssetDecoder, ChannelPolicy, PreparedBms,
    asset_paths::AssetPathPolicy,
    asset_source::AssetSource,
    prepare_from_source,
    step_gameplay::{StepGameplay, StepLocalGameplay, StepGameplayConfig},
    local_players::{PlayerId, ResolvedInputPlan},
    mine_audio_consumers_fixtures::{recorded, Action, replay_limits},
    replay_audio::{plan_audio, plan_section_audio},
    offline::{render_offline, OfflineOptions},
    competition::{Competition, OpponentKind},
};
use beatkernel::{
    audio::{
        AudioFormat, AudioLimits, AudioCommand, Mixer, MixerConfig, PcmLimits, PcmSample,
        SampleBank, CommandProducer, SampleId, command_queue,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::HazardOutcome,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::BmsInputMode;
use std::{
    borrow::Cow,
    cell::RefCell,
    error::Error,
    io,
    path::{Path, PathBuf},
};
#[derive(Default)]
struct Assets {
    names: RefCell<Vec<String>>,
    reads: RefCell<Vec<PathBuf>>,
    decoded: RefCell<Vec<PathBuf>>,
}
impl AssetSource for Assets {
    fn resolve(&self, name: &str, policy: AssetPathPolicy) -> io::Result<PathBuf> {
        assert_eq!(policy, AssetPathPolicy::Exact);
        self.names.borrow_mut().push(name.into());
        if matches!(name, "blast.pcm" | "note.pcm") {
            Ok(name.into())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "unexpected prepared resource",
            ))
        }
    }
    fn read<'a>(&'a self, path: &Path, bound: usize) -> io::Result<Cow<'a, [u8]>> {
        assert_eq!(bound, 64 * 1024 * 1024);
        self.reads.borrow_mut().push(path.into());
        Ok(Cow::Borrowed(if path == Path::new("blast.pcm") {
            &[0]
        } else {
            &[1]
        }))
    }
}
impl AssetDecoder for Assets {
    fn decode(
        &self,
        path: &Path,
        bytes: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>> {
        self.decoded.borrow_mut().push(path.into());
        let pcm = if path == Path::new("blast.pcm") {
            assert_eq!(bytes, [0]);
            vec![0.5, -0.25]
        } else {
            assert_eq!(path, Path::new("note.pcm"));
            assert_eq!(bytes, [1]);
            vec![1., -1.]
        };
        Ok(PcmSample::new(AudioFormat::new(10, 1)?, pcm, limits)?)
    }
}
fn limits() -> PcmLimits {
    PcmLimits::new(64, 512, 8).unwrap()
}
fn load(text: &str, assets: &Assets) -> PreparedBms {
    prepare_from_source(
        text.as_bytes(),
        assets,
        AudioFormat::new(10, 1).unwrap(),
        limits(),
        ChannelPolicy::Exact,
        assets,
        AssetPathPolicy::Exact,
        0,
        None,
    )
    .unwrap()
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
struct Domains;
impl ClockMapper for Domains {
    fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        (point.domain == target).then_some(point.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(1, 0),
        output_origin: point(2, 0),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 8,
        bgm_pending: 4,
        bgm_lookahead: Duration::from_nanos(1_000_000_000),
        telemetry_capacity: 8,
    }
}
fn bindings() -> BindingMap {
    bindings_for(DeviceSelector::Any)
}
fn bindings_for(device: DeviceSelector) -> BindingMap {
    BindingMap::from_bindings([Binding {
        device,
        physical: PhysicalControlId::keyboard(91),
        game_control: GameControlId(0x11),
    }])
    .unwrap()
}
fn input(device: u64, ns: i64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(device), point(1, ns), 1),
        control: PhysicalControlId::keyboard(91),
        state: ButtonState::Down,
    })
}
fn mixer(bank: SampleBank, origin: i64) -> (CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(8).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            bank.format(),
            ClockDomainId(2),
            Timestamp::from_nanos(origin),
            AudioLimits::new(8, 4, 8, 32, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}
fn deliver(game: &mut StepGameplay, producer: &mut CommandProducer) {
    while let Some(batch) = game.take_commands(8).unwrap() {
        for command in &batch.commands {
            producer.try_push(*command).unwrap();
        }
        game.acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
    }
}
#[test]
fn actual_loader_step_trigger_or_avoid_uses_optional_zero_pcm_damage_and_real_mixer() {
    for trigger in [false, true] {
        let assets = Assets::default();
        let prepared = load(
            "#BPM 60\n#VOLWAV 50\n#WAV00 blast.pcm\n#000D1:01\n",
            &assets,
        );
        assert_eq!(*assets.names.borrow(), ["blast.pcm"]);
        assert_eq!(assets.decoded.borrow().len(), 1);
        let (mut game, bank) =
            StepGameplay::new_section(prepared, config(), bindings(), Timestamp::ZERO, None)
                .unwrap();
        game.activate(point(1, 0)).unwrap();
        let report = if trigger {
            game.process_input(input(1, 0), &Domains, point(2, 0))
                .unwrap()
        } else {
            game.advance_to(point(1, 0), &Domains, point(2, 0)).unwrap()
        };
        assert_eq!(
            report.hazard_events[0].outcome,
            if trigger {
                HazardOutcome::Triggered
            } else {
                HazardOutcome::Avoided
            }
        );
        assert_eq!(report.hazard_events[0].value, 1);
        assert!(report.judge_events.is_empty());
        assert_eq!(
            (game.mine_damage().triggered, game.mine_damage().avoided),
            if trigger { (1, 0) } else { (0, 1) }
        );
        assert_eq!(
            game.gauge().snapshot().level_units,
            if trigger { 19_500_000 } else { 20_000_000 }
        );
        assert_eq!(game.score().hits, 0);
        let (mut producer, mut output) = mixer(bank, 0);
        deliver(&mut game, &mut producer);
        let mut pcm = [0.; 4];
        let rendered = output.render(&mut pcm).unwrap();
        assert_eq!(
            pcm,
            if trigger {
                [0.25, -0.125, 0., 0.]
            } else {
                [0.; 4]
            }
        );
        assert_eq!(rendered.counters.unknown_samples, 0);
    }
}
#[test]
fn fatal_only_and_undefined_zero_are_silent_no_acquisition_and_fatal_fences_actual_owner() {
    for (row, fatal) in [("#WAV00 never.pcm\n#000D1:ZZ", true), ("#000D1:01", false)] {
        let assets = Assets::default();
        let prepared = load(&format!("#BPM 60\n{row}\n"), &assets);
        assert!(assets.names.borrow().is_empty());
        assert!(assets.reads.borrow().is_empty());
        assert!(assets.decoded.borrow().is_empty());
        assert_eq!(prepared.bank.len(), 0);
        let (mut game, _) =
            StepGameplay::new_section(prepared, config(), bindings(), Timestamp::ZERO, None)
                .unwrap();
        game.activate(point(1, 0)).unwrap();
        let report = game
            .process_input(input(1, 0), &Domains, point(2, 0))
            .unwrap();
        assert_eq!(report.hazard_events[0].outcome, HazardOutcome::Triggered);
        assert!(report.audio_commands.is_empty());
        assert_eq!(game.mine_damage().instant_death, fatal);
        assert_eq!(
            game.gauge().snapshot().level_units,
            if fatal { 0 } else { 19_500_000 }
        );
        assert_eq!(game.gameplay_fence().is_some(), fatal);
        let later = game
            .advance_to(point(1, 1_000_000_000), &Domains, point(2, 1_000_000_000))
            .unwrap();
        assert!(later.hazard_events.is_empty());
        assert_eq!(game.mine_damage().triggered, 1);
    }
}
#[test]
fn actual_loaded_local_players_have_independent_occupancy_damage_and_disjoint_mine_voices() {
    for both in [false, true] {
        let assets = Assets::default();
        let prepared = load("#BPM 60\n#WAV00 blast.pcm\n#000D1:0001\n", &assets);
        let ids = [PlayerId(u32::MAX), PlayerId(7)];
        let plan = ResolvedInputPlan::new(vec![
            (ids[0], Some(DeviceId(1))),
            (ids[1], Some(DeviceId(2))),
        ])
        .unwrap();
        let (mut game, bank) = StepLocalGameplay::new_section(
            prepared,
            config(),
            plan,
            vec![
                bindings_for(DeviceSelector::Exact(DeviceId(1))),
                bindings_for(DeviceSelector::Exact(DeviceId(2))),
            ],
            Timestamp::ZERO,
            None,
            BmsInputMode::ButtonOnly,
        )
        .unwrap();
        game.activate(point(1, 0)).unwrap();
        game.process_input(input(1, 0), &Domains, point(2, 0))
            .unwrap();
        if both {
            game.process_input(input(2, 0), &Domains, point(2, 0))
                .unwrap();
        }
        let reports = game
            .advance_to(point(1, 2_000_000_000), &Domains, point(2, 2_000_000_000))
            .unwrap();
        assert_eq!(
            reports.iter().map(|row| row.player).collect::<Vec<_>>(),
            ids
        );
        assert_eq!(game.mine_damage(ids[0]).unwrap().triggered, 1);
        assert_eq!(game.mine_damage(ids[1]).unwrap().triggered, u64::from(both));
        assert_eq!(game.mine_damage(ids[1]).unwrap().avoided, u64::from(!both));
        let voices: Vec<_> = reports
            .iter()
            .flat_map(|row| &row.report.audio_commands)
            .filter_map(|command| match command {
                AudioCommand::Play {
                    sample: SampleId(0),
                    voice,
                    ..
                } => Some(*voice),
                _ => None,
            })
            .collect();
        assert_eq!(voices.len(), if both { 2 } else { 1 });
        if both {
            assert_ne!(voices[0], voices[1]);
        }
        let (mut producer, mut output) = mixer(bank, 2_000_000_000);
        while let Some(batch) = game.take_commands(8).unwrap() {
            for command in &batch.commands {
                producer.try_push(*command).unwrap();
            }
            game.acknowledge(batch.sequence, batch.commands.len(), true)
                .unwrap();
        }
        let mut pcm = [0.; 2];
        output.render(&mut pcm).unwrap();
        assert_eq!(pcm, if both { [1., -0.5] } else { [0.5, -0.25] });
        assert_eq!(assets.decoded.borrow().len(), 1);
    }
}
#[test]
fn admitted_capture_replay_and_saved_competition_share_hazards_hash_and_reject_changed_damage_before_io()
 {
    let text = "#BPM 60\n#WAV00 blast.pcm\n#000D1:0101\n";
    let assets = Assets::default();
    let prepared = load(text, &assets);
    let actual = recorded(
        &prepared,
        &[Action::Press(0, 91), Action::Advance(2_500_000_000)],
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    assert_eq!(
        actual
            .reports
            .iter()
            .map(|report| report.hazard_events.len())
            .sum::<usize>(),
        2
    );
    assert!(actual.reports[1].hazard_events[0].input.is_none());
    let plan = plan_audio(
        &prepared,
        actual.file.clone(),
        replay_limits(),
        point(2, 1_000_000_000),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(plan.final_judge_hash, actual.hash);
    assert_eq!(plan.commands, actual.commands);
    assert!(plan.judge_events.is_empty());
    let mut reconstructed =
        crate::replay_playback::reconstruct(&prepared.source, actual.file.clone(), replay_limits())
            .unwrap();
    reconstructed
        .seek_cursor(actual.file.records.len())
        .unwrap();
    assert_eq!(reconstructed.engine().stable_hash().unwrap(), actual.hash);
    let mut competition = Competition::new(actual.file.header.clone(), 1).unwrap();
    competition
        .add_replay(
            &prepared.source,
            actual.file.clone(),
            replay_limits(),
            OpponentKind::Own,
            "mines",
        )
        .unwrap();
    competition
        .observe(&[], Timestamp::from_nanos(9_007_199_254_740_993))
        .unwrap();
    assert_eq!(
        (
            competition.opponents()[0].score().hits,
            competition.opponents()[0].score().misses
        ),
        (0, 0)
    );
    let changed = Assets::default();
    let error = prepare_from_source(
        text.replace(":0101", ":0201").as_bytes(),
        &changed,
        AudioFormat::new(10, 1).unwrap(),
        limits(),
        ChannelPolicy::Exact,
        &changed,
        AssetPathPolicy::Exact,
        0,
        Some((&actual.file, replay_limits())),
    )
    .unwrap_err();
    assert!(
        error
            .downcast_ref::<crate::replay_playback::PlaybackError>()
            .is_some()
    );
    assert!(changed.names.borrow().is_empty());
    assert!(changed.reads.borrow().is_empty());
    assert!(changed.decoded.borrow().is_empty());
}
#[test]
fn actual_loaded_finite_practice_retains_original_marker_time_identity_and_zero_sample() {
    let assets = Assets::default();
    let original = load("#BPM 60\n#WAV00 blast.pcm\n#000D1:00010100\n", &assets);
    let zero = original.bank.get(SampleId(0)).unwrap().samples().as_ptr();
    let (selected, _) =
        crate::section_start::prepare_at(original, Timestamp::from_nanos(1_500_000_000), limits())
            .unwrap();
    assert_eq!(
        selected.bank.get(SampleId(0)).unwrap().samples().as_ptr(),
        zero
    );
    let markers = selected.source.compile_mines().unwrap();
    assert_eq!(markers.len(), 2);
    assert_eq!(
        (markers[0].ordinal, markers[0].at.as_nanos()),
        (0, 1_000_000_000)
    );
    assert_eq!(
        (markers[1].ordinal, markers[1].at.as_nanos()),
        (1, 2_000_000_000)
    );
    let actual = recorded(
        &selected,
        &[
            Action::Press(1_500_000_000, 91),
            Action::Advance(2_500_000_000),
        ],
        BmsInputMode::ButtonOnly,
        1_500_000_000,
        Some(3_000_000_000),
        0,
        0,
    );
    assert_eq!(
        actual.reports[1].hazard_events[0].at.as_nanos(),
        2_000_000_000
    );
    assert!(
        plan_audio(
            &selected,
            actual.file.clone(),
            replay_limits(),
            point(2, 1_000_000_000),
            Duration::ZERO
        )
        .is_err()
    );
    let plan = plan_section_audio(
        &selected,
        actual.file,
        replay_limits(),
        point(2, 1_000_000_000),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(plan.final_judge_hash, actual.hash);
    assert!(plan.commands.iter().any(|command| matches!(
        command,
        AudioCommand::Play {
            sample: SampleId(0),
            ..
        }
    )));
}
#[test]
fn source_only_mine_prevents_completion_until_actual_hazard_and_output_frontier_are_drained() {
    let assets = Assets::default();
    let prepared = load("#BPM 60\n#000D1:0001\n", &assets);
    let (mut game, bank) =
        StepGameplay::new_section(prepared, config(), bindings(), Timestamp::ZERO, None).unwrap();
    game.activate(point(1, 0)).unwrap();
    game.advance_to(point(1, 0), &Domains, point(2, 0)).unwrap();
    let (_producer, mut output) = mixer(bank, 0);
    let first = output.render(&mut [0.; 1]).unwrap();
    assert!(
        !game
            .observe_completion(Some(first), Some(point(2, 100_000_000)))
            .unwrap()
    );
    assert!(game.completed_result().is_none());
    game.advance_to(point(1, 2_000_000_000), &Domains, point(2, 2_000_000_000))
        .unwrap();
    assert_eq!(game.mine_damage().avoided, 1);
    // Completion requires moving strictly beyond the inclusive last marker.
    game.advance_to(point(1, 2_000_000_001), &Domains, point(2, 2_000_000_001))
        .unwrap();
    for (frames, ns) in [(20usize, 2_100_000_000), (1, 2_200_000_000)] {
        let report = output.render(&mut vec![0.; frames]).unwrap();
        game.observe_completion(Some(report), Some(point(2, ns)))
            .unwrap();
    }
    assert!(game.completed_result().is_some());
    assert_eq!((game.score().hits, game.score().misses), (0, 0));
    assert!(assets.names.borrow().is_empty());
}
#[test]
fn actual_loaded_offline_note_and_mine_render_shared_gain_pcm_without_treating_damage_as_sample_id()
{
    let assets = Assets::default();
    let prepared = load(
        "#BPM 60\n#VOLWAV 50\n#WAV00 blast.pcm\n#WAV01 note.pcm\n#00011:01\n#000D1:01\n",
        &assets,
    );
    assert_eq!(*assets.names.borrow(), ["blast.pcm", "note.pcm"]);
    let mut bytes = Vec::new();
    let report = render_offline(
        prepared,
        OfflineOptions {
            frames: 4,
            block_frames: 2,
            command_capacity: 8,
            max_voices: 4,
        },
        &mut bytes,
    )
    .unwrap();
    let samples: Vec<_> = bytes
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect();
    assert_eq!(samples, [0.75, -0.625, 0., 0.]);
    assert_eq!(
        (report.hits, report.judge_results, report.frames),
        (1, 1, 4)
    );
    assert_eq!(report.last_render.unwrap().counters.unknown_samples, 0);
}

#[cfg(feature = "graphics")]
#[test]
fn actual_loader_mine_only_chart_reaches_retained_render_packets_without_normal_note_progress() {
    use crate::{
        player_chart::PlayerChart, note_progress::NoteProgress, playfield_gpu::PlayfieldCache,
        ui::interaction::Bounds,
    };
    use std::sync::Arc;
    let assets = Assets::default();
    let prepared = load("#BPM 60\n#WAV00 blast.pcm\n#000D1:01ZZ\n", &assets);
    let chart =
        Arc::new(PlayerChart::from_compiled(&prepared.source, &prepared.compiled.chart).unwrap());
    assert!(chart.notes.is_empty());
    assert_eq!(chart.mines().len(), 2);
    assert_eq!(NoteProgress::new(chart.clone()).unwrap().state(0), None);
    let mut indices = Vec::new();
    chart
        .visible_mine_indices_checked(Timestamp::ZERO, 4_000_000_000, 0, &mut indices)
        .unwrap();
    assert_eq!(indices, [0, 1]);
    let bounds = Bounds {
        x: 24,
        y: 110,
        width: 640,
        height: 515,
    };
    let mut cache = PlayfieldCache::default();
    let first = cache.frame_indexed_with_mines_and_progress(
        &chart.notes,
        &[],
        chart.mines(),
        &indices,
        chart.lanes.len(),
        bounds,
        Timestamp::ZERO,
        4_000_000_000,
        None,
    );
    assert_eq!(first.instances.len(), 2);
    assert!(
        first
            .instances
            .iter()
            .all(|instance| instance.appearance[0] == 2.)
    );
    assert_ne!(first.instances[0].appearance, first.instances[1].appearance);
    let same = cache.frame_indexed_with_mines_and_progress(
        &chart.notes,
        &[],
        chart.mines(),
        &indices,
        chart.lanes.len(),
        bounds,
        Timestamp::ZERO,
        4_000_000_000,
        None,
    );
    assert!(Arc::ptr_eq(&first.instances, &same.instances));
    assert_eq!(assets.decoded.borrow().len(), 1);
}
