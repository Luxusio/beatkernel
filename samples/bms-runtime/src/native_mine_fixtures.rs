//! Deferred native mine-construction fixtures using actual portable owners.
//! No source-admission, gauge, native device or OS-loop acceptance is implied.
use crate::{
    competition_live::CompetitionOptions,
    local_players::PlayerId,
    local_runtime::{MemberConfig, SoloRuntime},
    mine_plan::prepare_judge,
    native_group_competition::canonical_identity,
    native_judge::{
        NativeJudgeConfig, capture_limits, prepare_capture, prepare_capture_for_source,
    },
    replay_playback::{PlaybackError, decode_section_setup, reconstruct, validate_setup},
};
use beatkernel::{
    audio::{CommandProducer, command_queue},
    chart::Beat,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, ContactId, DeviceId,
        DeviceSelector, EventMeta, GameControlId, GameInputEvent, PhysicalControlId,
        PhysicalInputEvent, Position2, TouchEvent, TouchPhase,
    },
    judge::{
        HazardEvent, HazardId, HazardOutcome, JudgeEngine, JudgeError, JudgeGrade, JudgeOutcome,
    },
    replay::codec::{ReplayCodecLimits, decode_replay},
    runtime::RuntimeProcessingClock,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsChart, BmsInputMode, MineDamage, parse};

const HOST: ClockDomainId = ClockDomainId(17);
const OUTPUT: ClockDomainId = ClockDomainId(23);
const ORIGIN: i64 = 10_000_000_000;
const TEXT: &str = "#BPM 60\n#000D1:011EZZ02";

fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn point(domain: ClockDomainId, value: i64) -> ClockPoint {
    ClockPoint {
        domain,
        timestamp: ts(value),
    }
}
fn config() -> NativeJudgeConfig {
    NativeJudgeConfig {
        early: 11,
        late: 23,
        offset: 0,
        preroll: 0,
        output: OUTPUT,
        end: None,
    }
}
fn source() -> BmsChart {
    parse(TEXT, Default::default()).unwrap()
}
fn judge(source: &BmsChart) -> JudgeEngine {
    config()
        .judge(source, source.compile().unwrap().chart)
        .unwrap()
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 32, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn bindings(device: DeviceSelector, key: u16) -> BindingMap {
    BindingMap::from_bindings([Binding {
        device,
        physical: PhysicalControlId::keyboard(key),
        game_control: GameControlId(0x11),
    }])
    .unwrap()
}
fn meta(device: u64, song: i64, sequence: u64) -> EventMeta {
    let mut meta = EventMeta::new(DeviceId(device), point(HOST, ORIGIN + song), sequence);
    meta.original_clock_point = Some(point(ClockDomainId(99), 9_007_199_254_740_993));
    meta
}
fn button(device: u64, song: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: meta(device, song, sequence),
        control: PhysicalControlId::keyboard(91),
        state,
    })
}
fn bound(physical: PhysicalInputEvent) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(0x11),
        physical,
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
struct NoMapping;
impl ClockMapper for NoMapping {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn solo(judge: JudgeEngine, producer: CommandProducer) -> SoloRuntime {
    let mut runtime = SoloRuntime::new(
        HOST,
        OUTPUT,
        Transport::new(ts(ORIGIN - 1), ts(-1), Rate::NORMAL),
        bindings(DeviceSelector::Any, 91),
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    runtime
}

#[test]
fn native_constructor_preserves_legacy_bytes_and_actual_asymmetric_signed_profile() {
    let ordinary = parse(
        "#BPM 60\n#WAV01 key\n#00011:01\n#000D1:00",
        Default::default(),
    )
    .unwrap();
    let mut unused_grid = ordinary.clone();
    unused_grid.mine_ticks_per_beat = 0;
    for source in [&ordinary, &unused_grid] {
        for offset in [-19, 19] {
            let mut cfg = config();
            cfg.offset = offset;
            let chart = source.compile().unwrap().chart;
            let legacy =
                JudgeEngine::new(chart.clone(), source.rules(), cfg.profile().unwrap()).unwrap();
            let native = cfg.judge(source, chart).unwrap();
            assert_eq!(native.stable_hash().unwrap(), legacy.stable_hash().unwrap());
            let old = prepare_capture(&legacy, HOST, Timestamp::ZERO, 0, Some(limits()))
                .unwrap()
                .unwrap()
                .into_bytes()
                .unwrap();
            let new = prepare_capture_for_source(
                source,
                &native,
                HOST,
                Timestamp::ZERO,
                0,
                Some(limits()),
            )
            .unwrap()
            .unwrap()
            .into_bytes()
            .unwrap();
            assert_eq!(
                new, old,
                "empty mines retain the complete legacy recording bytes"
            );
        }
    }

    let source = parse(
        "#BPM 60\n#WAV01 key\n#00011:01\n#000D1:1E",
        Default::default(),
    )
    .unwrap();
    for offset in [-19, 19] {
        let mut cfg = config();
        cfg.offset = offset;
        let native = cfg.judge(&source, source.compile().unwrap().chart).unwrap();
        let common = prepare_judge(
            &source,
            source.compile().unwrap().chart,
            cfg.profile().unwrap(),
            BmsInputMode::ButtonOnly,
            100_000,
        )
        .unwrap();
        assert_eq!(native.stable_hash().unwrap(), common.stable_hash().unwrap());
        for delta in [-11, 23] {
            let mut native = cfg.judge(&source, source.compile().unwrap().chart).unwrap();
            let song = delta - offset;
            let input = bound(button(u64::MAX, song, 7, ButtonState::Down));
            let hits = native.push_input(&input, ts(song)).unwrap();
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].at, ts(delta));
            assert_eq!(
                hits[0].outcome,
                JudgeOutcome::Hit {
                    grade: JudgeGrade(1),
                    delta: Duration::from_nanos(delta),
                }
            );
        }
        let mut native = cfg.judge(&source, source.compile().unwrap().chart).unwrap();
        assert!(native.advance_to(ts(23 - offset)).unwrap().is_empty());
        assert_eq!(
            native.hazard_events(),
            &[hazard(0, 0, 50, HazardOutcome::Avoided, None)]
        );
        assert_eq!(native.advance_to(ts(24 - offset)).unwrap().len(), 1);
        assert!(native.hazard_events().is_empty());

        let mut native = cfg.judge(&source, source.compile().unwrap().chart).unwrap();
        let input = bound(button(3, -offset, u64::MAX, ButtonState::Down));
        assert_eq!(native.push_input(&input, ts(-offset)).unwrap().len(), 1);
        assert_eq!(
            native.hazard_events(),
            &[hazard(
                0,
                0,
                50,
                HazardOutcome::Triggered,
                Some(*input.physical.meta()),
            )]
        );
    }
}

#[test]
fn native_solo_reports_real_button_occupancy_without_enabling_contact_or_automatic_audio() {
    let source = source();
    let (producer, _consumer) = command_queue(8).unwrap();
    let mut runtime = solo(judge(&source), producer);
    for sequence in [0, 1] {
        let report = runtime
            .process_input(
                button(3, -1, sequence, ButtonState::Down),
                &NoMapping,
                point(OUTPUT, 100),
            )
            .unwrap();
        assert!(report.hazard_events.is_empty());
        assert!(report.judge_events.is_empty() && report.judge_error.is_none());
    }
    // Releasing the first of two actual owners cannot clear the second one;
    // duplicate Down and Repeat must not require extra releases.
    for (song, device, sequence, state, id, value, outcome) in [
        (
            0,
            u64::MAX,
            2,
            ButtonState::Down,
            0,
            1,
            HazardOutcome::Triggered,
        ),
        (
            1_000_000_000,
            3,
            3,
            ButtonState::Up,
            1,
            50,
            HazardOutcome::Triggered,
        ),
        (
            2_000_000_000,
            u64::MAX,
            4,
            ButtonState::Repeat,
            2,
            1295,
            HazardOutcome::Triggered,
        ),
        (
            3_000_000_000,
            u64::MAX,
            5,
            ButtonState::Up,
            3,
            2,
            HazardOutcome::Avoided,
        ),
    ] {
        let input = button(device, song, sequence, state);
        let original_meta = *input.meta();
        let report = runtime
            .process_input(
                input.clone(),
                &NoMapping,
                point(OUTPUT, 500 + sequence as i64),
            )
            .unwrap();
        assert_eq!(report.song_time, ts(song));
        assert_eq!(report.bound_inputs, vec![bound(input)]);
        assert_eq!(
            report.hazard_events,
            vec![hazard(id, song, value, outcome, Some(original_meta))]
        );
        assert!(report.judge_events.is_empty() && report.judge_error.is_none());
        assert!(report.audio_commands.is_empty() && report.audio_failures.is_empty());
    }
    assert!(
        runtime
            .advance_to(
                point(HOST, ORIGIN + 3_000_000_001),
                &NoMapping,
                point(OUTPUT, 999)
            )
            .unwrap()
            .hazard_events
            .is_empty()
    );

    let (producer, _consumer) = command_queue(8).unwrap();
    let mut runtime = solo(judge(&source), producer);
    let touch = PhysicalInputEvent::Touch(TouchEvent {
        meta: meta(u64::MAX, 0, u64::MAX),
        control: PhysicalControlId::keyboard(91),
        contact: ContactId(u64::MAX),
        phase: TouchPhase::Down,
        position: Position2 { x: -1.0, y: 17.5 },
        pressure: Some(0.5),
    });
    assert!(!runtime.judge().is_fresh_press(&bound(touch.clone())));
    let report = runtime
        .process_input(touch.clone(), &NoMapping, point(OUTPUT, 1))
        .unwrap();
    assert_eq!(report.bound_inputs, vec![bound(touch.clone())]);
    assert_eq!(
        report.hazard_events,
        vec![hazard(0, 0, 1, HazardOutcome::Avoided, Some(*touch.meta()))]
    );
    assert!(report.judge_events.is_empty() && report.audio_commands.is_empty());
}

fn members(source: &BmsChart, key: u16) -> Vec<MemberConfig> {
    (0..2)
        .map(|index| {
            let device = DeviceId(u64::MAX - index);
            MemberConfig {
                player: PlayerId(7 + index as u32),
                device: Some(device),
                bindings: bindings(DeviceSelector::Exact(device), key),
                judge: judge(source),
                sounds: vec![],
            }
        })
        .collect()
}
fn identity(
    source: &BmsChart,
    members: &[MemberConfig],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    canonical_identity(
        &CompetitionOptions::default(),
        source,
        members,
        HOST,
        Timestamp::ZERO,
        0,
        None,
        0,
    )
}

#[test]
fn actual_native_capture_reconstructs_hazards_and_competition_uses_the_configured_initial_hash() {
    let source = source();
    let mut cfg = config();
    cfg.end = Some(ts(4_000_000_000));
    let judge = cfg.judge(&source, source.compile().unwrap().chart).unwrap();
    let pristine = judge.stable_hash().unwrap();
    let mut capture =
        prepare_capture_for_source(&source, &judge, HOST, Timestamp::ZERO, 0, Some(limits()))
            .unwrap()
            .unwrap();
    let setup = decode_section_setup(&capture.header().options).unwrap();
    assert_eq!(setup.input_mode, BmsInputMode::ButtonOnly);
    assert_eq!(
        setup.end, None,
        "native recording keeps its established end=None layout"
    );
    let (producer, _consumer) = command_queue(8).unwrap();
    let mut runtime = solo(judge, producer);
    let first = button(u64::MAX, 0, 0, ButtonState::Down);
    let report = runtime
        .process_input(first.clone(), &NoMapping, point(OUTPUT, 101))
        .unwrap();
    assert_eq!(
        report.hazard_events,
        vec![hazard(
            0,
            0,
            1,
            HazardOutcome::Triggered,
            Some(*first.meta())
        )]
    );
    capture.record_report(&report).unwrap();
    let report = runtime
        .advance_to(
            point(HOST, ORIGIN + 1_000_000_000),
            &NoMapping,
            point(OUTPUT, 102),
        )
        .unwrap();
    assert_eq!(
        report.hazard_events,
        vec![hazard(1, 1_000_000_000, 50, HazardOutcome::Triggered, None)]
    );
    capture.record_report(&report).unwrap();
    let release = button(u64::MAX, 2_000_000_000, 1, ButtonState::Up);
    let report = runtime
        .process_input(release.clone(), &NoMapping, point(OUTPUT, 103))
        .unwrap();
    assert_eq!(
        report.hazard_events,
        vec![hazard(
            2,
            2_000_000_000,
            1295,
            HazardOutcome::Avoided,
            Some(*release.meta())
        )]
    );
    capture.record_report(&report).unwrap();
    let report = runtime
        .advance_to(
            point(HOST, ORIGIN + 3_000_000_000),
            &NoMapping,
            point(OUTPUT, 104),
        )
        .unwrap();
    assert_eq!(
        report.hazard_events,
        vec![hazard(3, 3_000_000_000, 2, HazardOutcome::Avoided, None)]
    );
    capture.record_report(&report).unwrap();
    let file = decode_replay(&capture.into_bytes().unwrap(), limits()).unwrap();
    assert_eq!(file.records.len(), 4);
    assert_eq!(
        validate_setup(&source, &file, limits())
            .unwrap()
            .stable_hash()
            .unwrap(),
        pristine
    );
    let replay = reconstruct(&source, file.clone(), limits()).unwrap();
    assert_eq!(
        replay.engine().stable_hash().unwrap(),
        runtime.judge().stable_hash().unwrap()
    );
    assert_eq!(
        replay.engine().hazard_events(),
        runtime.judge().hazard_events()
    );

    let original_members = members(&source, 91);
    let original_identity = identity(&source, &original_members).unwrap();
    assert_eq!(
        identity(&source, &members(&source, 17)).unwrap(),
        original_identity
    );
    let mut value = source.clone();
    value.mines[0].damage = MineDamage::from_raw(2).unwrap();
    let mut time = source.clone();
    time.mines[0].beat = Beat::new(8).unwrap();
    let mut ordinal = source.clone();
    ordinal.mines[0].ordinal = u64::MAX;
    let mut lane = source.clone();
    lane.mines[0].lane = parse("#BPM 60\n#000D2:01", Default::default())
        .unwrap()
        .mines[0]
        .lane;
    for changed in [value, time, ordinal, lane] {
        assert!(matches!(
            validate_setup(&changed, &file, limits()),
            Err(PlaybackError::IdentityMismatch(_))
        ));
        assert!(identity(&changed, &original_members).is_err());
        assert_ne!(
            identity(&changed, &members(&changed, 91)).unwrap(),
            original_identity
        );
    }
    assert_eq!(original_members[0].judge.stable_hash().unwrap(), pristine);
}

#[test]
fn native_setup_refuses_invalid_profiles_mines_and_default_capacity_before_returning_an_owner() {
    let source = source();
    let original = source.clone();
    let pristine = judge(&source).stable_hash().unwrap();
    for (early, late) in [(-1, 23), (11, -1)] {
        let mut cfg = config();
        cfg.early = early;
        cfg.late = late;
        let error = cfg
            .judge(&source, source.compile().unwrap().chart)
            .err()
            .unwrap();
        assert_eq!(
            error.downcast_ref::<JudgeError>(),
            Some(&JudgeError::InvalidProfile)
        );
    }
    let mut grid = source.clone();
    grid.mine_ticks_per_beat = 0;
    let mut position = source.clone();
    position.mines[1].beat = position.mines[0].beat;
    let mut ordinal = source.clone();
    ordinal.mines[1].ordinal = ordinal.mines[0].ordinal;
    let mut overflow = source.clone();
    overflow.mines[1].beat = Beat::new(i64::MAX).unwrap();
    for invalid in [grid, position, ordinal, overflow] {
        let before = invalid.clone();
        assert!(
            config()
                .judge(&invalid, invalid.compile().unwrap().chart)
                .is_err()
        );
        assert_eq!(invalid, before);
    }
    let mut over_capacity = source.clone();
    over_capacity.mines = vec![source.mines[0]; 100_001];
    over_capacity.mine_ticks_per_beat = 0;
    let error = config()
        .judge(&over_capacity, over_capacity.compile().unwrap().chart)
        .err()
        .unwrap();
    assert!(
        error.to_string().contains("capacity"),
        "the fixed native count budget precedes mine compilation"
    );
    assert_eq!(source, original);
    assert_eq!(judge(&source).stable_hash().unwrap(), pristine);

    // These actual optional-capture APIs still return before inspecting unused
    // settings, source metadata or the already-started judge. No output owner,
    // file, device or networking endpoint is manufactured in this fixture.
    assert!(capture_limits(false, 0, usize::MAX).unwrap().is_none());
    assert!(capture_limits(true, 0, 0).is_err());
    let mut started = judge(&source);
    started.advance_to(Timestamp::ZERO).unwrap();
    let hash = started.stable_hash().unwrap();
    assert!(
        prepare_capture_for_source(&over_capacity, &started, HOST, ts(-1), u64::MAX, None)
            .unwrap()
            .is_none()
    );
    assert!(
        prepare_capture_for_source(&source, &started, HOST, Timestamp::ZERO, 0, Some(limits()))
            .is_err()
    );
    assert_eq!(started.stable_hash().unwrap(), hash);
}
