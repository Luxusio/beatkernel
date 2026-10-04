//! Common local-player construction and recording finalization outside native callbacks.
pub use crate::native_finish::save_capture;
use crate::{
    PreparedBms,
    competition::ScoreSummary,
    competition_live::{CompetitionOptions, LiveCompetition},
    local_input::InputMerger,
    local_players::{MAX_LOCAL_PLAYERS, PlayerId, ResolvedInputPlan},
    local_preparation::{PreparedLocalMembers, prepare_local_members},
    local_runtime::{MemberConfig, RuntimeGroup},
    native_cohort::{PlayerState, member_progress, replay_path},
    native_group_competition::NativeGroupCompetition,
    native_gameplay::NativeGameplayResult,
    native_judge::{NativeJudgeConfig, capture_limits, prepare_capture},
    replay_capture::LiveReplayCapture,
};
use beatkernel::{
    audio::{CommandProducer, VoiceId},
    input::{Binding, BindingMap, DeviceId, DeviceSelector, GameControlId, PhysicalControlId},
    time::{ClockDomainId, ClockPoint, Timestamp},
    transport::Transport,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub fn admit_cohort(count: usize, _network: bool) -> NativeGameplayResult<()> {
    if !(2..=MAX_LOCAL_PLAYERS).contains(&count) {
        return Err("local play requires 2..64 assigned inputs".into());
    }
    Ok(())
}
pub struct CohortPreparation<'a> {
    pub host: ClockDomainId,
    pub output: ClockDomainId,
    pub early: i64,
    pub late: i64,
    pub offset: i64,
    pub preroll: i64,
    pub start: Timestamp,
    pub end: Option<Timestamp>,
    pub chart_seed: u64,
    pub bindings: &'a BTreeMap<u8, u16>,
    pub record_replay: Option<&'a Path>,
    pub replay_max_bytes: usize,
    pub replay_max_records: usize,
}
pub struct PreparedCohort {
    pub configs: Vec<MemberConfig>,
    pub states: Vec<PlayerState>,
    pub save_paths: Vec<(PlayerId, Option<PathBuf>)>,
    pub reserved: Vec<VoiceId>,
    pub network: Option<NativeGroupCompetition>,
}
/// Builds all pure per-member state before opponent loading can touch a file.
pub fn prepare_cohort(
    prepared: &PreparedBms,
    assignments: &[(PlayerId, DeviceId)],
    competition: &CompetitionOptions,
    config: &CohortPreparation<'_>,
) -> NativeGameplayResult<PreparedCohort> {
    admit_cohort(assignments.len(), false)?;
    if config.host == config.output
        || config.start.as_nanos() < 0
        || !(0..=10_000_000_000).contains(&config.preroll)
        || config.end.is_some_and(|end| end < config.start)
    {
        return Err("invalid cohort clock/section configuration".into());
    }
    let song_origin = Timestamp::from_nanos(i64::try_from(
        i128::from(config.start.as_nanos()) - i128::from(config.preroll),
    )?);
    let mut players = BTreeSet::new();
    let mut devices = BTreeSet::new();
    for &(player, device) in assignments {
        if player.0 == 0 || device.0 == 0 || !players.insert(player.0) || !devices.insert(device.0)
        {
            return Err("cohort assignments require distinct positive player/device IDs".into());
        }
    }
    let mut keys = BTreeSet::new();
    for (&lane, &key) in config.bindings {
        if !matches!(lane,0x11..=0x19|0x21..=0x29) || key == 0 || !keys.insert(key) {
            return Err("invalid/duplicate cohort lane binding".into());
        }
    }
    for note in &prepared.source.notes {
        if !config.bindings.contains_key(&note.lane.channel()) {
            return Err(
                format!("missing --bind for BMS channel{:02X}", note.lane.channel()).into(),
            );
        }
    }
    let judge_config = NativeJudgeConfig {
        early: config.early,
        late: config.late,
        offset: config.offset,
        preroll: config.preroll,
        output: config.output,
        end: config.end,
    };
    let profile = judge_config.profile()?;
    let limits = capture_limits(
        config.record_replay.is_some(),
        config.replay_max_bytes,
        config.replay_max_records,
    )?;
    let plan = ResolvedInputPlan::new(
        assignments
            .iter()
            .map(|&(player, device)| (player, Some(device)))
            .collect(),
    )?;
    let mut maps = Vec::new();
    maps.try_reserve_exact(assignments.len())?;
    for &(_, device) in assignments {
        maps.push(BindingMap::from_bindings(config.bindings.iter().map(
            |(&lane, &key)| Binding {
                device: DeviceSelector::Exact(device),
                physical: PhysicalControlId::keyboard(key),
                game_control: GameControlId(u32::from(lane)),
            },
        ))?);
    }
    let PreparedLocalMembers { configs, reserved } = prepare_local_members(
        prepared,
        &plan,
        maps,
        profile,
        beatkernel_bms::BmsInputMode::ButtonOnly,
    )?;
    let mut states = Vec::new();
    let mut save_paths = Vec::new();
    states.try_reserve_exact(assignments.len())?;
    save_paths.try_reserve_exact(assignments.len())?;
    for member in &configs {
        let player = member.player;
        let path = config
            .record_replay
            .map(|path| replay_path(path, player))
            .transpose()?;
        let capture = prepare_capture(
            &member.judge,
            config.host,
            config.start,
            config.chart_seed,
            limits,
        )?;
        let completion = judge_config.completion(prepared)?;
        states.push(PlayerState {
            player,
            capture,
            competition: None,
            completion,
            score: ScoreSummary::default(),
            last_song: song_origin,
        });
        save_paths.push((player, path));
    }
    let mut saved_options = competition.clone();
    saved_options.network = None;
    for (state, member) in states.iter_mut().zip(&configs) {
        state.competition = LiveCompetition::prepare_for_at_with_chart_seed(
            state.player,
            &saved_options,
            &prepared.source,
            &member.judge,
            config.host,
            config.start,
            config.chart_seed,
        )?;
    }
    let network = if competition.network.is_some() {
        NativeGroupCompetition::prepare(
            competition,
            &prepared.source,
            &configs,
            config.host,
            config.start,
            config.chart_seed,
            config.end,
            config.preroll,
        )?
    } else {
        None
    };
    Ok(PreparedCohort {
        network,
        configs,
        states,
        save_paths,
        reserved,
    })
}
/// Activate on the actual native epoch without replacing original device IDs.
pub fn activate_cohort(
    configs: Vec<MemberConfig>,
    reserved: &[VoiceId],
    host_origin: ClockPoint,
    output: ClockDomainId,
    transport: Transport,
    producer: CommandProducer,
    end: Option<Timestamp>,
) -> NativeGameplayResult<(RuntimeGroup, InputMerger)> {
    admit_cohort(configs.len(), false)?;
    if transport.anchor().host_time != host_origin.timestamp {
        return Err("cohort transport anchor differs from native host origin".into());
    }
    let devices = configs
        .iter()
        .map(|member| {
            member
                .device
                .ok_or("cohort member lacks assigned native device")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let merger = InputMerger::new(host_origin.domain, host_origin, devices, 65536)?;
    let mut group = RuntimeGroup::new(
        host_origin.domain,
        output,
        transport,
        producer,
        configs,
        4096,
        reserved,
    )?;
    if let Some(end) = end {
        group.set_song_end(end)?;
    }
    Ok((group, merger))
}
/// Call after both output and acquisition cleanup, before independent saves.
/// A malformed terminal snapshot still invokes owner cleanup and retains errors.
pub fn finish_cohort_network(
    network: Option<&mut NativeGroupCompetition>,
    states: &[PlayerState],
    failures: &mut Vec<String>,
) {
    if let Some(network) = network {
        finalize_network(states, failures, |members| network.finish(members));
    }
}

fn finalize_network(
    states: &[PlayerState],
    failures: &mut Vec<String>,
    mut finish: impl FnMut(&[crate::multiplayer_group::MemberProgress]) -> NativeGameplayResult<()>,
) {
    match member_progress(states) {
        Ok(members) => {
            if let Err(error) = finish(&members) {
                failures.push(format!("shared network cleanup: {error}"));
            }
        }
        Err(error) => {
            failures.push(format!("terminal cohort snapshot: {error}"));
            if let Err(error) = finish(&[]) {
                failures.push(format!("shared network cleanup: {error}"));
            }
        }
    }
}

#[cfg(test)]
#[path = "native_cohort_network_fixtures.rs"]
mod network_fixtures;

/// Call after every native cleanup attempt. Match by ID, never positional zip.
/// Ambiguous destinations are rejected rather than publishing a capture twice.
pub fn finish_cohort(
    mut states: Vec<PlayerState>,
    save_paths: Vec<(PlayerId, Option<PathBuf>)>,
    mut failures: Vec<String>,
    mut save: impl FnMut(Option<LiveReplayCapture>, Option<&Path>, bool) -> NativeGameplayResult<()>,
) -> NativeGameplayResult<()> {
    let failed_session = !failures.is_empty();
    if states.is_empty() {
        failures.push("empty final cohort roster".into());
    }
    let mut members = BTreeMap::<PlayerId, usize>::new();
    let mut paths = BTreeMap::<PlayerId, Vec<Option<PathBuf>>>::new();
    let mut publications = BTreeMap::<PathBuf, usize>::new();
    for state in &states {
        *members.entry(state.player).or_default() += 1;
    }
    for (player, path) in save_paths {
        if let Some(path) = &path {
            *publications.entry(path.clone()).or_default() += 1;
        }
        paths.entry(player).or_default().push(path);
    }
    for (player, count) in &members {
        if player.0 == 0 || *count != 1 {
            failures.push(format!("invalid/duplicate final member player{}", player.0));
        }
    }
    for (player, destinations) in &paths {
        if !members.contains_key(player) {
            failures.push(format!("unknown save destination player{}", player.0));
        }
        if destinations.len() != 1 {
            failures.push(format!("duplicate save destination player{}", player.0));
        }
    }
    for (path, count) in &publications {
        if *count > 1 {
            failures.push(format!("duplicate replay publication path {path:?}"));
        }
    }
    for state in &mut states {
        if let Some(competition) = state.competition.as_mut() {
            competition.finish();
        }
        if members[&state.player] != 1 || state.player.0 == 0 {
            continue;
        }
        let Some(destinations) = paths.get(&state.player) else {
            failures.push(format!("missing save destination player{}", state.player.0));
            continue;
        };
        if destinations.len() != 1 {
            continue;
        }
        let path = destinations[0].as_deref();
        if path.is_some_and(|path| publications.get(path).is_some_and(|count| *count > 1)) {
            continue;
        }
        if let Err(error) = save(state.capture.take(), path, failed_session) {
            failures.push(format!(
                "player{} replay save after cleanup: {error}",
                state.player.0
            ));
        }
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
    use beatkernel::{
        audio::*,
        input::{ButtonEvent, ButtonState, CodecLimits, EventMeta, PhysicalInputEvent},
        judge::JudgeStage,
        replay::codec::ReplayCodecLimits,
        runtime::SoundBinding,
        transport::Rate,
    };
    fn host(ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(1),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn prepared() -> PreparedBms {
        let source = beatkernel_bms::parse(
            "#BPM 3000\n#WAV01 original.wav\n#00011:00010000\n#00001:01\n",
            Default::default(),
        )
        .unwrap();
        let compiled = source.compile().unwrap();
        let format = AudioFormat::new(1000, 1).unwrap();
        let limits = PcmLimits::new(64, 256, 1).unwrap();
        let mut bank = SampleBank::new(format, limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25, 0.5], limits).unwrap(),
        )
        .unwrap();
        let sounds = vec![SoundBinding {
            object: compiled.chart.objects()[0].id,
            stage: JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(1),
            gain: 1.0,
        }];
        PreparedBms {
            source,
            compiled,
            bank,
            sounds,
            bgm_commands: vec![AudioCommand::Play {
                voice: VoiceId(99),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            }],
        }
    }
    fn config(bindings: &BTreeMap<u8, u16>) -> CohortPreparation<'_> {
        CohortPreparation {
            host: ClockDomainId(1),
            output: ClockDomainId(2),
            early: 0,
            late: 0,
            offset: 0,
            preroll: 0,
            start: Timestamp::ZERO,
            end: None,
            chart_seed: 3,
            bindings,
            record_replay: Some(Path::new("records/run.bkr")),
            replay_max_bytes: 65536,
            replay_max_records: 128,
        }
    }
    #[test]
    fn sparse_rosters_build_isolated_actual_members_and_capture_seed_without_io() {
        let prepared = prepared();
        let bindings = BTreeMap::from([(0x11, 4)]);
        for count in [2, 3, 4, 64] {
            let mut assignments: Vec<_> = (0..count)
                .map(|index| {
                    (
                        PlayerId(7 + index as u32 * 11),
                        DeviceId(1000 + index as u64),
                    )
                })
                .collect();
            assignments.last_mut().unwrap().0 = PlayerId(u32::MAX);
            let cohort = prepare_cohort(
                &prepared,
                &assignments,
                &CompetitionOptions::default(),
                &config(&bindings),
            )
            .unwrap();
            assert_eq!(cohort.reserved, vec![VoiceId(99)]);
            let mut voices = BTreeSet::new();
            for ((member, state), (player, device)) in
                cohort.configs.iter().zip(&cohort.states).zip(&assignments)
            {
                assert_eq!(member.player, *player);
                assert_eq!(member.device, Some(*device));
                assert!(
                    member
                        .bindings
                        .bindings()
                        .iter()
                        .all(|binding| binding.device == DeviceSelector::Exact(*device))
                );
                assert!(state.completion.is_some());
                assert!(state.competition.is_none());
                assert_eq!(state.last_song, Timestamp::ZERO);
                assert!(
                    member
                        .sounds
                        .iter()
                        .all(|sound| sound.voice.0 > 99 && voices.insert(sound.voice.0))
                );
                let (_, start, seed) = crate::replay_playback::decode_chart_setup(
                    &state.capture.as_ref().unwrap().header().options,
                )
                .unwrap();
                assert_eq!((start, seed), (Timestamp::ZERO, 3));
            }
            assert_eq!(
                cohort.save_paths.last().unwrap().1.as_deref(),
                Some(Path::new("records/run.p4294967295.bkr"))
            );
        }
    }
    #[test]
    fn activation_uses_actual_origin_devices_and_runtime_reports_reconstruct() {
        let prepared = prepared();
        let bindings = BTreeMap::from([(0x11, 4)]);
        let assignments = [
            (PlayerId(7), DeviceId(91)),
            (PlayerId(u32::MAX), DeviceId(19)),
        ];
        let cohort = prepare_cohort(
            &prepared,
            &assignments,
            &CompetitionOptions::default(),
            &config(&bindings),
        )
        .unwrap();
        let PreparedCohort {
            configs,
            mut states,
            reserved,
            ..
        } = cohort;
        let (producer, _consumer) = command_queue(8).unwrap();
        let (mut group, mut merger) = activate_cohort(
            configs,
            &reserved,
            host(100),
            ClockDomainId(2),
            Transport::new(host(100).timestamp, Timestamp::ZERO, Rate::NORMAL),
            producer,
            None,
        )
        .unwrap();
        let event = PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(91), host(20_000_100), 1),
            control: PhysicalControlId::keyboard(4),
            state: ButtonState::Down,
        });
        merger.admit(event, host(20_000_100)).unwrap();
        let input = merger.pop_ready(host(20_000_100)).unwrap().unwrap();
        struct Mapper;
        impl beatkernel::time::ClockMapper for Mapper {
            fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
                None
            }
            fn quality(&self) -> beatkernel::time::ClockMappingQuality {
                beatkernel::time::ClockMappingQuality::Unknown
            }
        }
        let result = group
            .process_input(
                input,
                &Mapper,
                ClockPoint {
                    domain: ClockDomainId(2),
                    timestamp: Timestamp::from_nanos(20_000_000),
                },
            )
            .unwrap();
        let crate::local_runtime::InputResult::Processed(reports) = result else {
            panic!("assigned original device ignored")
        };
        assert_eq!(reports[0].player, PlayerId(7));
        assert_eq!(reports[0].report.judge_events.len(), 1);
        let capture = states[0].capture.as_mut().unwrap();
        capture.record_report(&reports[0].report).unwrap();
        let limits =
            ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(65536, 32768).unwrap())
                .unwrap();
        crate::replay_playback::reconstruct(
            &prepared.source,
            beatkernel::replay::codec::ReplayFile::new(
                capture.header().clone(),
                capture.records().to_vec(),
            ),
            limits,
        )
        .unwrap();
        assert!(states[1].capture.as_ref().unwrap().records().is_empty());
    }
    #[test]
    fn activation_rejects_epoch_mismatch_and_pure_preparation_rejects_missing_bindings() {
        let prepared = prepared();
        let bindings = BTreeMap::from([(0x11, 4)]);
        let assignments = [(PlayerId(7), DeviceId(91)), (PlayerId(9), DeviceId(19))];
        let cohort = prepare_cohort(
            &prepared,
            &assignments,
            &CompetitionOptions::default(),
            &config(&bindings),
        )
        .unwrap();
        let (producer, _consumer) = command_queue(8).unwrap();
        let error = activate_cohort(
            cohort.configs,
            &cohort.reserved,
            host(100),
            ClockDomainId(2),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            producer,
            None,
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("anchor"));
        let no_bindings = BTreeMap::new();
        assert!(
            prepare_cohort(
                &prepared,
                &assignments,
                &CompetitionOptions::default(),
                &config(&no_bindings)
            )
            .err()
            .unwrap()
            .to_string()
            .contains("missing --bind")
        );
        assert!(admit_cohort(2, true).is_ok());
        assert!(admit_cohort(1, false).is_err());
        assert!(admit_cohort(65, false).is_err());
    }
    #[test]
    fn finite_configuration_and_invalid_pure_inputs_precede_missing_ghost_access() {
        let mut prepared = prepared();
        let bindings = BTreeMap::from([(0x11, 4)]);
        let assignments = [(PlayerId(7), DeviceId(91)), (PlayerId(9), DeviceId(19))];
        let mut cfg = config(&bindings);
        cfg.start = Timestamp::from_nanos(10);
        cfg.preroll = 20;
        cfg.end = Some(Timestamp::from_nanos(30));
        let cohort = prepare_cohort(
            &prepared,
            &assignments,
            &CompetitionOptions::default(),
            &cfg,
        )
        .unwrap();
        assert!(cohort.states.iter().all(
            |state| state.completion.is_none() && state.last_song == Timestamp::from_nanos(-10)
        ));
        let mut competition = CompetitionOptions::default();
        competition.ghosts.push((
            crate::competition::OpponentKind::Own,
            PathBuf::from("missing-never-opened.bkr"),
        ));
        for invalid in [
            [(PlayerId(7), DeviceId(91)), (PlayerId(7), DeviceId(19))],
            [(PlayerId(7), DeviceId(91)), (PlayerId(9), DeviceId(91))],
            [(PlayerId(0), DeviceId(91)), (PlayerId(9), DeviceId(19))],
        ] {
            assert!(
                prepare_cohort(&prepared, &invalid, &competition, &cfg)
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("assignments")
            );
        }
        cfg.end = Some(Timestamp::from_nanos(9));
        assert!(
            prepare_cohort(&prepared, &assignments, &competition, &cfg)
                .err()
                .unwrap()
                .to_string()
                .contains("section")
        );
        cfg.end = None;
        cfg.early = -1;
        assert!(prepare_cohort(&prepared, &assignments, &competition, &cfg).is_err());
        cfg.early = 0;
        prepared.bgm_commands[0] = AudioCommand::Play {
            voice: VoiceId(u64::MAX),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        };
        assert!(
            prepare_cohort(&prepared, &assignments, &competition, &cfg)
                .err()
                .unwrap()
                .to_string()
                .contains("namespace")
        );
    }
    fn states(ids: &[u32]) -> Vec<PlayerState> {
        ids.iter()
            .map(|id| PlayerState {
                player: PlayerId(*id),
                capture: None,
                competition: None,
                completion: None,
                score: ScoreSummary::default(),
                last_song: Timestamp::ZERO,
            })
            .collect()
    }
    #[test]
    fn finalization_matches_ids_attempts_all_and_preserves_cleanup_failure_status() {
        let mut calls = Vec::new();
        let result = finish_cohort(
            states(&[7, 99, u32::MAX]),
            vec![
                (PlayerId(99), Some("b.bkr".into())),
                (PlayerId(u32::MAX), Some("c.bkr".into())),
                (PlayerId(7), Some("a.bkr".into())),
            ],
            vec!["native stop failed".into()],
            |_, path, failed| {
                calls.push((path.unwrap().to_path_buf(), failed));
                if path == Some(Path::new("a.bkr")) {
                    Err("save failed".into())
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(
            calls,
            vec![
                (PathBuf::from("a.bkr"), true),
                (PathBuf::from("b.bkr"), true),
                (PathBuf::from("c.bkr"), true)
            ]
        );
        let error = result.unwrap_err().to_string();
        assert!(error.contains("native stop failed") && error.contains("player7 replay save"));
        let mut flags = Vec::new();
        finish_cohort(
            states(&[7, 99]),
            vec![(PlayerId(7), None), (PlayerId(99), None)],
            Vec::new(),
            |_, _, failed| {
                flags.push(failed);
                Err("individual save failure".into())
            },
        )
        .unwrap_err();
        assert_eq!(flags, vec![false, false]);
    }
    #[test]
    fn ambiguous_destinations_are_never_published_but_unambiguous_saves_continue() {
        let mut calls = Vec::new();
        let result = finish_cohort(
            states(&[1, 2, 3, 4]),
            vec![
                (PlayerId(1), Some("same.bkr".into())),
                (PlayerId(2), Some("same.bkr".into())),
                (PlayerId(3), Some("three.bkr".into())),
                (PlayerId(3), None),
                (PlayerId(4), Some("safe.bkr".into())),
                (PlayerId(99), None),
            ],
            Vec::new(),
            |_, path, _| {
                calls.push(path.unwrap().to_path_buf());
                Ok(())
            },
        );
        assert_eq!(calls, vec![PathBuf::from("safe.bkr")]);
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("duplicate replay publication")
                && error.contains("unknown save destination")
                && error.contains("duplicate save destination")
        );
        let mut calls = 0;
        assert!(
            finish_cohort(
                states(&[1, 1, 2]),
                vec![(PlayerId(1), None)],
                Vec::new(),
                |_, _, _| {
                    calls += 1;
                    Ok(())
                }
            )
            .is_err()
        );
        assert_eq!(calls, 0);
        assert!(
            finish_cohort(Vec::new(), Vec::new(), Vec::new(), |_, _, _| panic!(
                "empty roster has no save"
            ))
            .is_err()
        );
    }
}
