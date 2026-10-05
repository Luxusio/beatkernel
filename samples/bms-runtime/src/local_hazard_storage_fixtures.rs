//! Deferred actual loader/member construction and independent shared-runtime state.
use crate::{
    AssetDecoder, ChannelPolicy, PreparedBms, prepare_from_source,
    asset_paths::AssetPathPolicy,
    asset_source::AssetSource,
    local_players::{PlayerId, ResolvedInputPlan},
    local_preparation::{prepare_local_members, prepare_local_mine_sounds},
    local_runtime::{RuntimeGroup, InputResult},
    mine_plan::prepare_judge,
    mine_damage::MineDamageSummary,
    gauge::BmsGauge,
};
use beatkernel::{
    audio::{
        AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample, AudioCommand,
        command_queue,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeProfile, JudgeGrade, JudgeWindow},
    runtime::RuntimeProcessingClock,
    time::{ClockDomainId, ClockPoint, ClockMapper, ClockMappingQuality, Timestamp, Duration},
    transport::{Transport, Rate},
};
use beatkernel_bms::BmsInputMode;
use std::{
    borrow::Cow,
    cell::RefCell,
    error::Error,
    io,
    path::{Path, PathBuf},
    collections::BTreeSet,
};
const MINES: &str = "#BPM 60\n#WAV00 blast.pcm\n#WAV01 note.pcm\n#00011:01\n#000D1:0001\n";
#[derive(Default)]
struct Assets {
    reads: RefCell<Vec<PathBuf>>,
}
impl AssetSource for Assets {
    fn resolve(&self, name: &str, _: AssetPathPolicy) -> io::Result<PathBuf> {
        if matches!(name, "blast.pcm" | "note.pcm") {
            Ok(name.into())
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "unexpected asset"))
        }
    }
    fn read<'a>(&'a self, path: &Path, _: usize) -> io::Result<Cow<'a, [u8]>> {
        self.reads.borrow_mut().push(path.into());
        Ok(Cow::Borrowed(&[1]))
    }
}
impl AssetDecoder for Assets {
    fn decode(
        &self,
        path: &Path,
        bytes: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>> {
        assert_eq!(bytes, [1]);
        let scale = if path == Path::new("blast.pcm") {
            0.25
        } else {
            0.5
        };
        Ok(PcmSample::new(
            AudioFormat::new(10, 1)?,
            vec![scale, -scale],
            limits,
        )?)
    }
}
fn loaded(text: &str, assets: &Assets) -> PreparedBms {
    prepare_from_source(
        text.as_bytes(),
        assets,
        AudioFormat::new(10, 1).unwrap(),
        PcmLimits::new(64, 512, 8).unwrap(),
        ChannelPolicy::Exact,
        assets,
        AssetPathPolicy::Exact,
        0,
        None,
    )
    .unwrap()
}
fn profile() -> JudgeProfile {
    JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(u32::MAX),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::ZERO,
    )
    .unwrap()
}
fn plan(count: usize) -> ResolvedInputPlan {
    ResolvedInputPlan::new(
        (0..count)
            .map(|index| {
                (
                    PlayerId(u32::MAX - index as u32 * 17),
                    Some(DeviceId(u64::MAX - index as u64)),
                )
            })
            .collect(),
    )
    .unwrap()
}
fn maps(plan: &ResolvedInputPlan) -> Vec<BindingMap> {
    plan.members()
        .iter()
        .map(|&(_, device)| {
            BindingMap::from_bindings([Binding {
                device: DeviceSelector::Exact(device.unwrap()),
                physical: PhysicalControlId::keyboard(4),
                game_control: GameControlId(0x11),
            }])
            .unwrap()
        })
        .collect()
}
const ORIGIN: i64 = 9_007_199_254_740_993;
fn point(domain: u32, time: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(time),
    }
}
fn press(device: DeviceId) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(device, point(1, ORIGIN), u64::MAX),
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    })
}
struct SameDomain;
impl ClockMapper for SameDomain {
    fn map(&self, point: ClockPoint, domain: ClockDomainId) -> Option<Timestamp> {
        (point.domain == domain).then_some(point.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
#[test]
fn one_two_and_sixty_four_loaded_members_preserve_ids_routes_and_source_aware_pristine_hashes() {
    for count in [1, 2, 64] {
        for mode in [BmsInputMode::ButtonOnly, BmsInputMode::ButtonOrContact] {
            let assets = Assets::default();
            let prepared = loaded(MINES, &assets);
            let plan = plan(count);
            let members =
                prepare_local_members(&prepared, &plan, maps(&plan), profile(), mode).unwrap();
            assert_eq!(members.configs.len(), count);
            let mut voices = BTreeSet::new();
            for (member, &(id, device)) in members.configs.iter().zip(plan.members()) {
                assert_eq!((member.player, member.device), (id, device));
                assert_eq!(
                    member.bindings.bindings()[0].device,
                    DeviceSelector::Exact(device.unwrap())
                );
                let separately = prepare_judge(
                    &prepared.source,
                    prepared.compiled.chart.clone(),
                    profile(),
                    mode,
                    100,
                )
                .unwrap();
                assert_eq!(
                    member.judge.stable_hash().unwrap(),
                    separately.stable_hash().unwrap()
                );
                for sound in &member.sounds {
                    assert!(voices.insert(sound.voice));
                }
            }
            let mines =
                prepare_local_mine_sounds(&prepared, &members.configs, &members.reserved, &[])
                    .unwrap();
            assert_eq!(mines.len(), count);
            for (id, timeline) in &mines {
                assert!(plan.members().iter().any(|row| row.0 == *id));
                for binding in timeline.bindings() {
                    assert!(voices.insert(binding.voice));
                }
            }
            assert_eq!(voices.len(), count * 2);
            assert_eq!(assets.reads.borrow().len(), 2);
        }
    }
}
#[test]
fn shared_runtime_first_player_occupancy_and_other_avoids_match_independent_source_judges() {
    for count in [1, 2, 64] {
        let assets = Assets::default();
        let prepared = loaded(MINES, &assets);
        let plan = plan(count);
        let mut references: Vec<_> = plan
            .members()
            .iter()
            .map(|_| {
                prepare_judge(
                    &prepared.source,
                    prepared.compiled.chart.clone(),
                    profile(),
                    BmsInputMode::ButtonOnly,
                    100,
                )
                .unwrap()
            })
            .collect();
        let members = prepare_local_members(
            &prepared,
            &plan,
            maps(&plan),
            profile(),
            BmsInputMode::ButtonOnly,
        )
        .unwrap();
        let mines =
            prepare_local_mine_sounds(&prepared, &members.configs, &members.reserved, &[]).unwrap();
        let (producer, _consumer) = command_queue(128).unwrap();
        let mut group = RuntimeGroup::new(
            ClockDomainId(1),
            ClockDomainId(2),
            Transport::new(Timestamp::from_nanos(ORIGIN), Timestamp::ZERO, Rate::NORMAL),
            producer,
            members.configs,
            0,
            &members.reserved,
        )
        .unwrap();
        group.configure_hazard_sounds(mines).unwrap();
        group.set_processing_clock(RuntimeProcessingClock::Disabled);
        let InputResult::Processed(first) = group
            .process_input(
                press(plan.members()[0].1.unwrap()),
                &SameDomain,
                point(2, 0),
            )
            .unwrap()
        else {
            panic!("assigned source was ignored")
        };
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].player, plan.members()[0].0);
        references[0]
            .push_input(&first[0].report.bound_inputs[0], Timestamp::ZERO)
            .unwrap();
        let mut gauges: Vec<_> = (0..count).map(|_| BmsGauge::default()).collect();
        gauges[0]
            .observe(
                &first[0].report.judge_events,
                &first[0].report.hazard_events,
            )
            .unwrap();
        let reports = group
            .advance_to(
                point(1, ORIGIN + 2_000_000_000),
                &SameDomain,
                point(2, 2_000_000_000),
            )
            .unwrap();
        assert_eq!(
            reports.iter().map(|row| row.player).collect::<Vec<_>>(),
            plan.members().iter().map(|row| row.0).collect::<Vec<_>>()
        );
        for (index, report) in reports.iter().enumerate() {
            let mut damage = MineDamageSummary::default();
            damage.observe(&report.report.hazard_events).unwrap();
            assert_eq!(
                (damage.triggered, damage.avoided),
                if index == 0 { (1, 0) } else { (0, 1) }
            );
            gauges[index]
                .observe(&report.report.judge_events, &report.report.hazard_events)
                .unwrap();
            assert_eq!(
                gauges[index].snapshot().level_units,
                if index == 0 { 20_500_000 } else { 14_000_000 }
            );
            references[index]
                .advance_to(Timestamp::from_nanos(2_000_000_000))
                .unwrap();
            assert_eq!(
                group
                    .member_judge(report.player)
                    .unwrap()
                    .stable_hash()
                    .unwrap(),
                references[index].stable_hash().unwrap()
            );
        }
    }
}
#[test]
fn all_held_members_execute_disjoint_note_and_mine_commands_using_one_original_pcm_bank() {
    for count in [1, 2, 64] {
        let assets = Assets::default();
        let prepared = loaded(MINES, &assets);
        let plan = plan(count);
        let members = prepare_local_members(
            &prepared,
            &plan,
            maps(&plan),
            profile(),
            BmsInputMode::ButtonOnly,
        )
        .unwrap();
        let mines =
            prepare_local_mine_sounds(&prepared, &members.configs, &members.reserved, &[]).unwrap();
        let (producer, consumer) = command_queue(128).unwrap();
        let mut group = RuntimeGroup::new(
            ClockDomainId(1),
            ClockDomainId(2),
            Transport::new(Timestamp::from_nanos(ORIGIN), Timestamp::ZERO, Rate::NORMAL),
            producer,
            members.configs,
            0,
            &members.reserved,
        )
        .unwrap();
        group.configure_hazard_sounds(mines).unwrap();
        group.set_processing_clock(RuntimeProcessingClock::Disabled);
        let mut voices = BTreeSet::new();
        for &(_, device) in plan.members() {
            let InputResult::Processed(reports) = group
                .process_input(press(device.unwrap()), &SameDomain, point(2, 0))
                .unwrap()
            else {
                panic!("source ignored")
            };
            for command in &reports[0].report.audio_commands {
                if let AudioCommand::Play { voice, .. } = command {
                    assert!(voices.insert(*voice));
                }
            }
        }
        let reports = group
            .advance_to(
                point(1, ORIGIN + 2_000_000_000),
                &SameDomain,
                point(2, 2_000_000_000),
            )
            .unwrap();
        for report in &reports {
            for command in &report.report.audio_commands {
                if let AudioCommand::Play { voice, .. } = command {
                    assert!(voices.insert(*voice));
                }
            }
        }
        assert_eq!(voices.len(), count * 2);
        let mut mixer = Mixer::new(
            MixerConfig::new(
                prepared.bank.format(),
                ClockDomainId(2),
                Timestamp::ZERO,
                AudioLimits::new(128, 128, 128, 32, 128).unwrap(),
            ),
            prepared.bank,
            consumer,
        )
        .unwrap();
        let mut pcm = [0.; 22];
        let rendered = mixer.render(&mut pcm).unwrap();
        assert_eq!(
            (pcm[0], pcm[1], pcm[20], pcm[21]),
            (
                count as f32 * 0.5,
                count as f32 * -0.5,
                count as f32 * 0.25,
                count as f32 * -0.25
            )
        );
        assert_eq!(rendered.counters.commands_consumed, count as u64 * 2);
        assert_eq!(rendered.counters.unknown_samples, 0);
        assert_eq!(assets.reads.borrow().len(), 2);
    }
}
#[test]
fn no_hazard_member_hashes_remain_legacy_and_invalid_binding_routes_are_refused_atomically() {
    let assets = Assets::default();
    let prepared = loaded(
        "#BPM 60\n#WAV00 unused.pcm\n#WAV01 note.pcm\n#00011:01\n",
        &assets,
    );
    let plan = plan(2);
    let members = prepare_local_members(
        &prepared,
        &plan,
        maps(&plan),
        profile(),
        BmsInputMode::ButtonOnly,
    )
    .unwrap();
    let plain = JudgeEngine::new(
        prepared.compiled.chart.clone(),
        prepared.source.rules(),
        profile(),
    )
    .unwrap();
    for member in &members.configs {
        assert_eq!(
            member.judge.stable_hash().unwrap(),
            plain.stable_hash().unwrap()
        );
        assert!(member.judge.hazard_events().is_empty());
    }
    let before = prepared.source.clone();
    assert!(
        prepare_local_members(
            &prepared,
            &plan,
            vec![],
            profile(),
            BmsInputMode::ButtonOnly
        )
        .is_err()
    );
    let wrong: Vec<_> = plan
        .members()
        .iter()
        .map(|_| {
            BindingMap::from_bindings([Binding {
                device: DeviceSelector::Any,
                physical: PhysicalControlId::keyboard(4),
                game_control: GameControlId(0x11),
            }])
            .unwrap()
        })
        .collect();
    assert!(
        prepare_local_members(&prepared, &plan, wrong, profile(), BmsInputMode::ButtonOnly)
            .is_err()
    );
    assert_eq!(prepared.source, before);
    assert_eq!(assets.reads.borrow().len(), 1);
}
