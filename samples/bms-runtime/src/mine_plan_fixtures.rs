//! Deferred typed mine composition. This bypasses the retained source-admission
//! guard solely to exercise actual owners; it is not completed BMS hazard play.
use crate::{
    PreparedBms,
    local_players::{PlayerId, ResolvedInputPlan},
    local_preparation::prepare_local_members,
    local_runtime::InputResult,
    mine_plan::{MinePlan, prepare_judge},
    offline::{OfflineOptions, render_offline},
    replay_capture::setup_input_header,
    replay_playback::{PlaybackError, reconstruct_section, validate_section_setup},
    section_start::source_at,
    step_gameplay::{StepGameplay, StepGameplayConfig, StepLocalGameplay},
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits, SampleBank},
    chart::{Beat, Bpm, BpmChange},
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, ContactId, DeviceId,
        DeviceSelector, EventMeta, GameControlId, GameInputEvent, PhysicalControlId,
        PhysicalInputEvent, Position2, TouchEvent, TouchPhase,
    },
    judge::{
        HazardEvent, HazardId, HazardMarker, HazardOutcome, JudgeEngine, JudgeGrade, JudgeProfile,
        JudgeWindow,
    },
    replay::{
        ReplayHeader,
        codec::{ReplayCodecLimits, ReplayFile, decode_replay},
    },
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::{BmsChart, BmsInputMode, MineDamage, parse};
use std::fmt::Write;

const MINES: &str = "#BPM 60\n#000D1:1EZZ";
const CONTACT: BmsInputMode = BmsInputMode::ButtonOrContact;
const BUTTON: BmsInputMode = BmsInputMode::ButtonOnly;
const HOST: i64 = 10_000_000_000;
const OUTPUT: i64 = 20_000_000_000;
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(domain: u32, n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(n),
    }
}
fn profile() -> JudgeProfile {
    JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::ZERO,
    )
    .unwrap()
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 64, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn source() -> BmsChart {
    parse(MINES, Default::default()).unwrap()
}
fn judge(source: &BmsChart, mode: BmsInputMode) -> JudgeEngine {
    prepare_judge(
        source,
        source.compile().unwrap().chart,
        profile(),
        mode,
        100,
    )
    .unwrap()
}
fn header(source: &BmsChart, start: i64, mode: BmsInputMode) -> ReplayHeader {
    setup_input_header(
        &judge(source, mode),
        ClockDomainId(1),
        limits(),
        ts(start),
        0,
        None,
        mode,
    )
    .unwrap()
}
fn meta(device: u64, elapsed: i64, sequence: u64) -> EventMeta {
    let mut meta = EventMeta::new(DeviceId(device), point(1, HOST + elapsed), sequence);
    meta.original_clock_point = Some(point(99, 9_007_199_254_740_993));
    meta
}
fn touch(device: u64, elapsed: i64, sequence: u64, phase: TouchPhase) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: meta(device, elapsed, sequence),
        control: PhysicalControlId::keyboard(91),
        contact: ContactId(u64::MAX),
        phase,
        position: Position2 { x: -3.0, y: 800.5 },
        pressure: Some(0.5),
    })
}
fn bound(input: PhysicalInputEvent) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(0x11),
        physical: input,
    }
}
fn hazard(
    id: u64,
    at: i64,
    value: u64,
    outcome: HazardOutcome,
    input: Option<EventMeta>,
) -> HazardEvent {
    HazardEvent {
        id: HazardId(id),
        at: ts(at),
        control: GameControlId(0x11),
        value,
        outcome,
        input,
    }
}
fn bindings(selector: DeviceSelector, lane: u32) -> BindingMap {
    BindingMap::from_bindings([Binding {
        device: selector,
        physical: PhysicalControlId::keyboard(91),
        game_control: GameControlId(lane),
    }])
    .unwrap()
}
fn prepared(source: BmsChart) -> PreparedBms {
    let compiled = source.compile().unwrap();
    assert!(compiled.chart.objects().is_empty() && compiled.bgm.is_empty());
    PreparedBms {
        source,
        compiled,
        bank: SampleBank::new(
            AudioFormat::new(10, 1).unwrap(),
            PcmLimits::new(64, 128, 4).unwrap(),
        )
        .unwrap(),
        sounds: vec![],
        bgm_commands: vec![],
    }
}
fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(1, HOST),
        output_origin: point(2, OUTPUT),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 8,
        bgm_pending: 1,
        bgm_lookahead: Duration::from_nanos(1_000_000_000),
        telemetry_capacity: 0,
    }
}
fn local_plan() -> ResolvedInputPlan {
    ResolvedInputPlan::new(vec![
        (PlayerId(9), Some(DeviceId(3))),
        (PlayerId(u32::MAX), Some(DeviceId(u64::MAX))),
    ])
    .unwrap()
}
fn local_bindings(lane: u32) -> Vec<BindingMap> {
    [3, u64::MAX]
        .map(|id| bindings(DeviceSelector::Exact(DeviceId(id)), lane))
        .into()
}
struct NoMapping;
impl ClockMapper for NoMapping {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}

#[test]
fn exact_plan_preserves_all_eighteen_lanes_damage_identity_original_timing_and_owned_timeline() {
    let mut text = "#BASE 62\n#BPM 60\n#WAV00 optional-explosion.wav\n".to_owned();
    for channel in [
        0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6,
        0xe7, 0xe8, 0xe9,
    ] {
        writeln!(
            text,
            "#000{channel:02X}:{}",
            if channel == 0xe9 { "zz" } else { "1e" }
        )
        .unwrap();
    }
    let source = parse(&text, Default::default()).unwrap();
    let plan = MinePlan::prepare(&source, 18).unwrap();
    let timeline = plan.timeline().unwrap();
    let lanes = [
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26,
        0x27, 0x28, 0x29,
    ];
    assert_eq!(plan.markers().len(), 18);
    for (index, lane) in lanes.into_iter().enumerate() {
        let value = if index == 17 { 1295 } else { 50 };
        assert_eq!(
            timeline.markers()[index],
            HazardMarker {
                id: HazardId(index as u64),
                at: ts(0),
                control: GameControlId(lane),
                value
            }
        );
        let mine = plan.markers()[index];
        assert_eq!(
            (
                mine.at,
                mine.lane.channel(),
                mine.damage.raw(),
                mine.ordinal,
                mine.line
            ),
            (ts(0), lane as u8, value as u16, index as u64, index + 4)
        );
    }
    assert!(plan.markers()[17].damage.is_fatal());
    assert_eq!(plan.markers()[0].damage.half_percent_units(), Some(50));
    assert!(source.notes.is_empty() && source.compile().unwrap().chart.objects().is_empty());
    let before = plan.markers().to_vec();
    let mut changed = source.clone();
    changed.mines[0].damage = MineDamage::from_raw(1294).unwrap();
    assert_eq!(plan.markers(), before);
    assert_eq!(plan.timeline().unwrap().markers(), timeline.markers());
    assert_eq!(
        MinePlan::prepare(&changed, 18).unwrap().markers()[0]
            .damage
            .raw(),
        1294
    );

    let timed = parse("#BPM 120\n#BPM01 240\n#STOP01 48\n#00002:0.75\n#00008:000100\n#00009:000100\n#000D1:011EZZ\n#001E1:ZY",
        Default::default()).unwrap();
    let plan = MinePlan::prepare(&timed, 4).unwrap();
    assert_eq!(
        plan.markers()
            .iter()
            .map(|m| (m.at.as_nanos(), m.damage.raw(), m.ordinal, m.line))
            .collect::<Vec<_>>(),
        [
            (0, 1, 0, 7),
            (500_000_000, 50, 1, 7),
            (1_000_000_000, 1295, 2, 7),
            (1_250_000_000, 1294, 3, 8)
        ]
    );
    let mut full = parse("#BPM 1\n#00002:2520\n#001E9:ZZ", Default::default()).unwrap();
    full.mines[0].ordinal = u64::MAX;
    assert_eq!(
        MinePlan::prepare(&full, 1)
            .unwrap()
            .timeline()
            .unwrap()
            .markers(),
        [HazardMarker {
            id: HazardId(u64::MAX),
            at: ts(604_800_000_000_000),
            control: GameControlId(0x29),
            value: 1295
        }]
    );
}

#[test]
fn capacity_precedes_timing_and_corrupt_typed_sources_never_return_partial_plans_or_judges() {
    let original = source();
    let before = original.clone();
    let capacity = MinePlan::prepare(&original, 1).unwrap_err();
    let mut corrupt = original.clone();
    corrupt.mine_ticks_per_beat = 0;
    assert_eq!(
        MinePlan::prepare(&corrupt, 1).unwrap_err(),
        capacity,
        "count refusal occurs before corrupt timing is examined"
    );
    assert!(MinePlan::prepare(&corrupt, 2).is_err());
    assert!(MinePlan::prepare(&original, 0).is_err());
    assert_eq!(
        MinePlan::prepare(&original, usize::MAX)
            .unwrap()
            .markers()
            .len(),
        2
    );
    let mut cases = vec![corrupt];
    let mut bad = original.clone();
    bad.source.ticks_per_beat = 0;
    cases.push(bad);
    let mut bad = original.clone();
    bad.source.ticks_per_beat = 2;
    bad.mine_ticks_per_beat = 3;
    cases.push(bad);
    let mut bad = original.clone();
    bad.mines[1].ordinal = bad.mines[0].ordinal;
    cases.push(bad);
    let mut bad = original.clone();
    bad.mines[1].beat = bad.mines[0].beat;
    cases.push(bad);
    let mut bad = original.clone();
    bad.mine_ticks_per_beat = 2;
    bad.source.bpm_changes.push(BpmChange {
        beat: Beat::new(i64::MAX).unwrap(),
        bpm: Bpm::new(60, 1).unwrap(),
    });
    cases.push(bad);
    let mut bad = original.clone();
    bad.mines[1].beat = Beat::new(i64::MAX).unwrap();
    cases.push(bad);
    for invalid in cases {
        let retained = invalid.clone();
        assert!(MinePlan::prepare(&invalid, 2).is_err());
        assert!(
            prepare_judge(
                &invalid,
                original.compile().unwrap().chart,
                profile(),
                CONTACT,
                2
            )
            .is_err()
        );
        assert_eq!(invalid, retained);
    }
    assert_eq!(original, before);
    assert_eq!(MinePlan::prepare(&original, 2).unwrap().markers().len(), 2);
}

#[test]
fn shared_judge_helper_preserves_empty_legacy_identity_and_owns_mine_only_contacts_with_reusable_snapshots()
 {
    for text in ["#BPM 60", "#BPM 60\n#WAV01 key.wav\n#00011:01"] {
        let mut source = parse(text, Default::default()).unwrap();
        source.mine_ticks_per_beat = 0; // Unused new metadata must not add a legacy validation gate.
        assert!(MinePlan::prepare(&source, 0).unwrap().timeline().is_none());
        for mode in [BUTTON, CONTACT] {
            let compiled = source.compile().unwrap().chart;
            let legacy = JudgeEngine::new(
                compiled.clone(),
                source.rules_with_input_mode(mode),
                profile(),
            )
            .unwrap();
            let actual = prepare_judge(&source, compiled, profile(), mode, 0).unwrap();
            assert_eq!(actual.stable_hash().unwrap(), legacy.stable_hash().unwrap());
            assert_eq!(
                setup_input_header(&actual, ClockDomainId(1), limits(), ts(0), 0, None, mode)
                    .unwrap(),
                setup_input_header(&legacy, ClockDomainId(1), limits(), ts(0), 0, None, mode)
                    .unwrap()
            );
            assert!(actual.hazard_events().is_empty());
        }
    }
    let source = source();
    for mode in [BUTTON, CONTACT] {
        let mut actual = judge(&source, mode);
        let down = bound(touch(u64::MAX, 0, u64::MAX, TouchPhase::Down));
        assert_eq!(actual.is_fresh_press(&down), mode == CONTACT);
        assert!(actual.push_input(&down, ts(0)).unwrap().is_empty());
        let outcome = if mode == CONTACT {
            HazardOutcome::Triggered
        } else {
            HazardOutcome::Avoided
        };
        assert_eq!(
            actual.hazard_events(),
            [hazard(0, 0, 50, outcome, Some(meta(u64::MAX, 0, u64::MAX)))]
        );
        let checkpoint = actual.snapshot().unwrap();
        let hash = actual.stable_hash().unwrap();
        for _ in 0..2 {
            let mut restored = JudgeEngine::from_snapshot(&checkpoint).unwrap();
            let cancel = bound(touch(u64::MAX, 2_000_000_000, u64::MAX, TouchPhase::Cancel));
            assert!(
                restored
                    .push_input(&cancel, ts(2_000_000_000))
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                restored.hazard_events(),
                [hazard(
                    1,
                    2_000_000_000,
                    1295,
                    HazardOutcome::Avoided,
                    Some(meta(u64::MAX, 2_000_000_000, u64::MAX))
                )]
            );
            restored.restore(&checkpoint).unwrap();
            assert_eq!(restored.stable_hash().unwrap(), hash);
        }
        assert_eq!(actual.stable_hash().unwrap(), hash);
    }
    let mut button_only = judge(&source, BUTTON);
    let input = bound(PhysicalInputEvent::Button(ButtonEvent {
        meta: meta(1, 0, 1),
        control: PhysicalControlId::keyboard(91),
        state: ButtonState::Down,
    }));
    button_only.push_input(&input, ts(0)).unwrap();
    assert_eq!(
        button_only.hazard_events(),
        [hazard(
            0,
            0,
            50,
            HazardOutcome::Triggered,
            Some(meta(1, 0, 1))
        )]
    );
}

#[test]
fn actual_stepped_solo_and_local_members_install_the_plan_capture_it_and_keep_occupancy_independent()
 {
    let original = source();
    let expected_header = header(&original, 0, CONTACT);
    let (mut solo, bank) = StepGameplay::new_section_with_input_mode(
        prepared(original.clone()),
        config(),
        bindings(DeviceSelector::Any, 0x11),
        ts(0),
        None,
        CONTACT,
    )
    .unwrap();
    assert_eq!(bank.len(), 0);
    assert_eq!(
        solo.competition_header(limits(), 0).unwrap(),
        expected_header
    );
    solo.configure_capture(limits(), 0).unwrap();
    solo.activate(config().host_origin).unwrap();
    let first = solo
        .process_input(
            touch(3, 0, 1, TouchPhase::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap();
    assert_eq!(
        first.hazard_events,
        [hazard(
            0,
            0,
            50,
            HazardOutcome::Triggered,
            Some(meta(3, 0, 1))
        )]
    );
    let release = solo
        .process_input(
            touch(3, 2_000_000_000, 2, TouchPhase::Cancel),
            &NoMapping,
            point(2, OUTPUT + 2_000_000_000),
        )
        .unwrap();
    assert_eq!(
        release.hazard_events,
        [hazard(
            1,
            2_000_000_000,
            1295,
            HazardOutcome::Avoided,
            Some(meta(3, 2_000_000_000, 2))
        )]
    );
    for report in [&first, &release] {
        assert!(
            report.judge_events.is_empty()
                && report.audio_commands.is_empty()
                && report.audio_failures.is_empty()
        );
    }
    let terminal = solo.judge().stable_hash().unwrap();
    solo.fail();
    let file = decode_replay(&solo.take_replay().unwrap().unwrap(), limits()).unwrap();
    assert_eq!(file.header, expected_header);
    assert_eq!(file.records.len(), 2);
    assert_eq!(
        reconstruct_section(&original, file, limits())
            .unwrap()
            .engine()
            .stable_hash()
            .unwrap(),
        terminal
    );

    let prepared = prepared(original.clone());
    assert!(
        prepare_local_members(
            &prepared,
            &local_plan(),
            local_bindings(0x12),
            profile(),
            CONTACT
        )
        .is_err()
    );
    let members = prepare_local_members(
        &prepared,
        &local_plan(),
        local_bindings(0x11),
        profile(),
        CONTACT,
    )
    .unwrap();
    assert_eq!(
        members.configs.iter().map(|m| m.player).collect::<Vec<_>>(),
        [PlayerId(9), PlayerId(u32::MAX)]
    );
    for member in &members.configs {
        assert_eq!(
            member.judge.stable_hash().unwrap(),
            judge(&original, CONTACT).stable_hash().unwrap()
        );
        assert!(member.sounds.is_empty());
    }
    let (mut local, bank) = StepLocalGameplay::new_section(
        prepared,
        config(),
        local_plan(),
        local_bindings(0x11),
        ts(0),
        None,
        CONTACT,
    )
    .unwrap();
    assert_eq!(bank.len(), 0);
    for player in [PlayerId(9), PlayerId(u32::MAX)] {
        local.configure_capture(player, limits(), 0).unwrap();
    }
    local.activate(config().host_origin).unwrap();
    assert!(matches!(
        local
            .process_input(
                touch(8, 0, 1, TouchPhase::Down),
                &NoMapping,
                point(2, OUTPUT)
            )
            .unwrap(),
        InputResult::Ignored {
            device: DeviceId(8)
        }
    ));
    let InputResult::Processed(first) = local
        .process_input(
            touch(3, 0, 1, TouchPhase::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap()
    else {
        panic!("actual assigned source must select its member")
    };
    assert_eq!(first[0].player, PlayerId(9));
    assert_eq!(
        first[0].report.hazard_events,
        [hazard(
            0,
            0,
            50,
            HazardOutcome::Triggered,
            Some(meta(3, 0, 1))
        )]
    );
    assert!(
        local
            .judge(PlayerId(u32::MAX))
            .unwrap()
            .hazard_events()
            .is_empty()
    );
    let advanced = local
        .advance_to(
            point(1, HOST + 2_000_000_000),
            &NoMapping,
            point(2, OUTPUT + 2_000_000_000),
        )
        .unwrap();
    assert_eq!(
        advanced[0].report.hazard_events,
        [hazard(
            1,
            2_000_000_000,
            1295,
            HazardOutcome::Triggered,
            None
        )]
    );
    assert_eq!(
        advanced[1].report.hazard_events,
        [
            hazard(0, 0, 50, HazardOutcome::Avoided, None),
            hazard(1, 2_000_000_000, 1295, HazardOutcome::Avoided, None)
        ]
    );
    let hashes = [
        local.judge(PlayerId(9)).unwrap().stable_hash().unwrap(),
        local
            .judge(PlayerId(u32::MAX))
            .unwrap()
            .stable_hash()
            .unwrap(),
    ];
    local.fail();
    for (index, player) in [PlayerId(9), PlayerId(u32::MAX)].into_iter().enumerate() {
        let file = decode_replay(&local.take_replay(player).unwrap().unwrap(), limits()).unwrap();
        assert_eq!(file.header, expected_header);
        assert_eq!(
            reconstruct_section(&original, file, limits())
                .unwrap()
                .engine()
                .stable_hash()
                .unwrap(),
            hashes[index]
        );
    }
}

#[test]
fn source_aware_replay_practice_and_offline_use_configured_hazards_without_claiming_damage_or_automatic_sound()
 {
    let original = source();
    let file = ReplayFile::new(header(&original, 0, CONTACT), vec![]);
    let pristine = validate_section_setup(&original, &file, limits()).unwrap();
    assert_eq!(
        pristine.stable_hash().unwrap(),
        judge(&original, CONTACT).stable_hash().unwrap()
    );
    let mut changes = Vec::new();
    let mut changed = original.clone();
    changed.mines[0].damage = MineDamage::from_raw(51).unwrap();
    changes.push(changed);
    let mut changed = original.clone();
    changed.mines[0].beat = Beat::new(1).unwrap();
    changes.push(changed);
    let mut changed = original.clone();
    changed.mines[0].ordinal = u64::MAX;
    changes.push(changed);
    let other_lane = parse("#000D2:01", Default::default()).unwrap().mines[0].lane;
    let mut changed = original.clone();
    changed.mines[0].lane = other_lane;
    changes.push(changed);
    let mut changed = original.clone();
    changed.mines.pop();
    changes.push(changed);
    for changed in changes {
        let before = changed.clone();
        assert!(matches!(
            validate_section_setup(&changed, &file, limits()),
            Err(PlaybackError::IdentityMismatch(_))
        ));
        assert_eq!(changed, before);
    }
    let mut provenance = original.clone();
    provenance.mines.reverse();
    for mine in &mut provenance.mines {
        mine.line += 99;
    }
    assert_eq!(header(&provenance, 0, CONTACT), file.header);
    let ordinary = JudgeEngine::new(
        original.compile().unwrap().chart,
        original.rules_with_input_mode(CONTACT),
        profile(),
    )
    .unwrap();
    let legacy = ReplayFile::new(
        setup_input_header(
            &ordinary,
            ClockDomainId(1),
            limits(),
            ts(0),
            0,
            None,
            CONTACT,
        )
        .unwrap(),
        vec![],
    );
    assert!(matches!(
        validate_section_setup(&original, &legacy, limits()),
        Err(PlaybackError::IdentityMismatch(_))
    ));
    let mut forged = file.clone();
    forged.header.chart_identity[0] ^= 1;
    assert!(validate_section_setup(&original, &forged, limits()).is_err());
    let mut bad = original.clone();
    bad.mine_ticks_per_beat = 0;
    assert!(matches!(
        validate_section_setup(&bad, &file, limits()),
        Err(PlaybackError::Hazards(_))
    ));

    // Constructor-only practice selection retains original mines. It does not
    // claim the full asset/practice admission policy has been integrated.
    let selected = source_at(&original, ts(1_000_000_000)).unwrap();
    assert_eq!(
        MinePlan::prepare(&selected, 2)
            .unwrap()
            .markers()
            .iter()
            .map(|m| m.at.as_nanos())
            .collect::<Vec<_>>(),
        [0, 2_000_000_000]
    );
    let (mut practice, _) = StepGameplay::new_section_with_input_mode(
        prepared(selected.clone()),
        config(),
        bindings(DeviceSelector::Any, 0x11),
        ts(1_000_000_000),
        None,
        CONTACT,
    )
    .unwrap();
    practice.configure_capture(limits(), 0).unwrap();
    practice.activate(config().host_origin).unwrap();
    let first = practice
        .process_input(
            touch(3, 0, 1, TouchPhase::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap();
    assert_eq!(first.song_time, ts(1_000_000_000));
    assert_eq!(
        first.hazard_events,
        [hazard(0, 0, 50, HazardOutcome::Avoided, None)]
    );
    let second = practice
        .advance_to(
            point(1, HOST + 1_000_000_000),
            &NoMapping,
            point(2, OUTPUT + 1_000_000_000),
        )
        .unwrap();
    assert_eq!(
        second.hazard_events,
        [hazard(
            1,
            2_000_000_000,
            1295,
            HazardOutcome::Triggered,
            None
        )]
    );
    let terminal = practice.judge().stable_hash().unwrap();
    practice.fail();
    let captured = decode_replay(&practice.take_replay().unwrap().unwrap(), limits()).unwrap();
    assert_eq!(
        reconstruct_section(&original, captured, limits())
            .unwrap()
            .engine()
            .stable_hash()
            .unwrap(),
        terminal
    );

    let options = OfflineOptions {
        frames: 30,
        block_frames: 7,
        command_capacity: 8,
        max_voices: 4,
    };
    let mut bytes = Vec::new();
    let report = render_offline(prepared(original), options, &mut bytes).unwrap();
    assert_eq!(
        (report.frames, report.hits, report.judge_results),
        (30, 0, 0)
    );
    assert_eq!(
        bytes,
        vec![0; 120],
        "hazards do not create synthetic presses or implicit explosion PCM"
    );
    let mut invalid = prepared(source());
    invalid.source.mine_ticks_per_beat = 0;
    let mut untouched = Vec::new();
    assert!(render_offline(invalid, options, &mut untouched).is_err());
    assert!(
        untouched.is_empty(),
        "the actual offline owner validates its mine plan before output publication"
    );
}
