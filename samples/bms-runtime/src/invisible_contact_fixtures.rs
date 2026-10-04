//! Deferred constructor/capture checks, not asset-admission or playable invisible BMS proof.
use crate::{
    PreparedBms,
    local_players::{PlayerId, ResolvedInputPlan},
    local_preparation::prepare_local_members,
    replay_playback::{decode_section_setup, reconstruct_section, validate_section_setup},
    section_start::prepare_at,
    step_gameplay::{StepGameplay, StepGameplayConfig},
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits, PcmSample, SampleBank, SampleId, VoiceId},
    input::{
        BackendId, Binding, BindingMap, CodecLimits, ContactId, DeviceId, DeviceSelector,
        EventMeta, GameControlId, GameInputEvent, PhysicalControlId, PhysicalInputEvent, Position2,
        TouchEvent, TouchPhase,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow},
    replay::{
        ReplayOperation,
        codec::{ReplayCodecLimits, decode_replay},
    },
    runtime::SoundBinding,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::{BmsChart, BmsInputMode, parse};

const INVISIBLE: &str = "#BPM 60\n#WAV01 key.wav\n#00031:01";
const MIXED: &str = "#BPM 60\n#WAV01 key.wav\n#00011:01\n#00031:01";
const VISIBLE: &str = "#BPM 60\n#WAV01 key.wav\n#00011:01";
const CONTACT: BmsInputMode = BmsInputMode::ButtonOrContact;
const BUTTON: BmsInputMode = BmsInputMode::ButtonOnly;
fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn point(domain: u32, value: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(value),
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
fn pcm_limits() -> PcmLimits {
    PcmLimits::new(64, 128, 4).unwrap()
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 64, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(1, 1_000_000_000),
        output_origin: point(2, 9_000_000_000),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 8,
        bgm_pending: 1,
        bgm_lookahead: Duration::from_nanos(1_000_000_000),
        telemetry_capacity: 4,
    }
}
fn surface() -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(0x544f5543),
        code: u32::MAX,
    }
}
fn bindings(device: DeviceSelector) -> BindingMap {
    BindingMap::from_bindings([Binding {
        device,
        physical: surface(),
        game_control: GameControlId(0x11),
    }])
    .unwrap()
}
fn touch(source: u64, elapsed: i64, sequence: u64, phase: TouchPhase) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(
        DeviceId(source),
        point(1, 1_000_000_000 + elapsed),
        sequence,
    );
    meta.original_clock_point = Some(point(99, 9_007_199_254_740_993));
    PhysicalInputEvent::Touch(TouchEvent {
        meta,
        control: surface(),
        contact: ContactId(u64::MAX),
        phase,
        position: Position2 { x: -3.0, y: 800.5 },
        pressure: Some(0.5),
    })
}
fn bound(source: u64, elapsed: i64, sequence: u64, phase: TouchPhase) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(0x11),
        physical: touch(source, elapsed, sequence, phase),
    }
}
fn prepared(text: &str, start: i64) -> (PreparedBms, BmsChart) {
    // Public typed composition isolates these constructor tests from the retained
    // prepare_from_source unsupported-invisible guard. No decoder/asset IO is used.
    let original = parse(text, Default::default()).unwrap();
    let source = original.clone();
    let compiled = source.compile().unwrap();
    let format = AudioFormat::new(8000, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits()).unwrap();
    if source.samples.contains_key(&1) {
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25, -0.25], pcm_limits()).unwrap(),
        )
        .unwrap();
    }
    let sounds = source
        .notes
        .iter()
        .map(|note| SoundBinding {
            object: note.object,
            stage: JudgeStage::Instant,
            sample: note.sample,
            voice: VoiceId(note.object.0),
            gain: 1.0,
        })
        .collect();
    assert!(compiled.bgm.is_empty());
    let value = PreparedBms {
        source,
        compiled,
        bank,
        sounds,
        bgm_commands: vec![],
    };
    (
        prepare_at(value, ts(start), pcm_limits()).unwrap().0,
        original,
    )
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
fn actual_solo_and_local_preparation_enable_only_the_contact_invisible_condition_including_filtered_empty()
 {
    for (text, start, mode, tracks, visible_count) in [
        (INVISIBLE, 0, CONTACT, true, 0),
        (MIXED, 1_000_000_000, CONTACT, true, 0),
        (VISIBLE, 1_000_000_000, CONTACT, false, 0),
        ("#BPM 60", 0, CONTACT, false, 0),
        (INVISIBLE, 0, BUTTON, false, 0),
        (MIXED, 1_000_000_000, BUTTON, false, 0),
        (VISIBLE, 0, CONTACT, true, 1),
    ] {
        let (prepared, _) = prepared(text, start);
        assert_eq!(prepared.compiled.chart.objects().len(), visible_count);
        assert_eq!(prepared.source.source.objects.len(), visible_count);
        let source = prepared.source.clone();
        let expected = if mode == CONTACT && !source.invisible.is_empty() {
            JudgeEngine::new_with_contacts(
                prepared.compiled.chart.clone(),
                source.rules_with_input_mode(mode),
                profile(),
            )
            .unwrap()
        } else {
            JudgeEngine::new(
                prepared.compiled.chart.clone(),
                source.rules_with_input_mode(mode),
                profile(),
            )
            .unwrap()
        };
        let plan = ResolvedInputPlan::new(vec![
            (PlayerId(1), Some(DeviceId(u64::MAX))),
            (PlayerId(2), Some(DeviceId(u64::MAX - 1))),
        ])
        .unwrap();
        let mut local = prepare_local_members(
            &prepared,
            &plan,
            vec![
                bindings(DeviceSelector::Exact(DeviceId(u64::MAX))),
                bindings(DeviceSelector::Exact(DeviceId(u64::MAX - 1))),
            ],
            profile(),
            mode,
        )
        .unwrap();
        let (owner, _bank) = StepGameplay::new_section_with_input_mode(
            prepared,
            config(),
            bindings(DeviceSelector::Any),
            ts(start),
            None,
            mode,
        )
        .unwrap();
        let input = bound(u64::MAX, 0, 1, TouchPhase::Down);
        assert_eq!(owner.judge().is_fresh_press(&input), tracks);
        assert_eq!(
            owner.judge().stable_hash().unwrap(),
            expected.stable_hash().unwrap()
        );
        assert_eq!(local.configs.len(), 2);
        for (index, member) in local.configs.iter().enumerate() {
            assert_eq!(member.player, PlayerId(index as u32 + 1));
            assert_eq!(
                member.judge.is_fresh_press(&bound(
                    u64::MAX - index as u64,
                    0,
                    1,
                    TouchPhase::Down
                )),
                tracks
            );
            assert_eq!(
                member.judge.stable_hash().unwrap(),
                expected.stable_hash().unwrap()
            );
        }
        if tracks {
            local.configs[0]
                .judge
                .push_input(&input, ts(start))
                .unwrap();
            assert!(!local.configs[0].judge.is_fresh_press(&input));
            assert!(local.configs[1].judge.is_fresh_press(&bound(
                u64::MAX - 1,
                0,
                1,
                TouchPhase::Down
            )));
            assert!(
                owner.judge().is_fresh_press(&input),
                "local ownership cannot mutate the independent solo judge"
            );
        }
    }
}

#[test]
fn actual_step_capture_reconstructs_empty_contact_ownership_and_practice_identity_without_fallback_installation()
 {
    for (text, start, tracks) in [
        (INVISIBLE, 0, true),
        (MIXED, 1_000_000_000, true),
        (VISIBLE, 1_000_000_000, false),
    ] {
        let (prepared, original) = prepared(text, start);
        assert!(prepared.compiled.chart.objects().is_empty());
        let (mut owner, _bank) = StepGameplay::new_section_with_input_mode(
            prepared,
            config(),
            bindings(DeviceSelector::Any),
            ts(start),
            None,
            CONTACT,
        )
        .unwrap();
        owner.configure_capture(limits(), u64::MAX).unwrap();
        owner.activate(config().host_origin).unwrap();
        let pristine = owner.judge().stable_hash().unwrap();
        let mut admitted = Vec::new();
        for (index, (phase, fresh)) in [
            (TouchPhase::Down, true),
            (TouchPhase::Down, false),
            (TouchPhase::Move, false),
            (TouchPhase::Cancel, false),
            (TouchPhase::Down, true),
            (TouchPhase::Up, false),
            (TouchPhase::Down, true),
        ]
        .into_iter()
        .enumerate()
        {
            let input = touch(u64::MAX, index as i64, index as u64 + 1, phase);
            let expected = GameInputEvent {
                game_control: GameControlId(0x11),
                physical: input.clone(),
            };
            assert_eq!(owner.judge().is_fresh_press(&expected), tracks && fresh);
            let report = owner
                .process_input(input, &NoMapping, point(2, 9_000_000_000 + index as i64))
                .unwrap();
            assert_eq!(report.song_time, ts(start + index as i64));
            assert_eq!(report.bound_inputs, [expected.clone()]);
            assert!(report.judge_events.is_empty() && report.judge_error.is_none());
            assert!(
                report.audio_commands.is_empty() && report.audio_failures.is_empty(),
                "constructor support does not install or schedule the still-unsupported BMS fallback plan"
            );
            admitted.push(expected);
        }
        let terminal = owner.judge().stable_hash().unwrap();
        owner.fail();
        let bytes = owner.take_replay().unwrap().unwrap();
        assert!(owner.take_replay().unwrap().is_none());
        let file = decode_replay(&bytes, limits()).unwrap();
        let setup = decode_section_setup(&file.header.options).unwrap();
        assert_eq!(setup.input_mode, CONTACT);
        assert_eq!(setup.start, ts(start));
        assert_eq!(setup.chart_seed, u64::MAX);
        assert_eq!(file.records.len(), 7);
        for (index, record) in file.records.iter().enumerate() {
            assert_eq!(record.song_time, ts(start + index as i64));
            assert_eq!(
                record.operation,
                ReplayOperation::Input(admitted[index].clone())
            );
        }
        let validated = validate_section_setup(&original, &file, limits()).unwrap();
        assert_eq!(validated.stable_hash().unwrap(), pristine);
        assert_eq!(
            validated.is_fresh_press(&bound(u64::MAX, 0, 1, TouchPhase::Down)),
            tracks
        );
        let mut replay = reconstruct_section(&original, file.clone(), limits()).unwrap();
        assert_eq!(replay.engine().stable_hash().unwrap(), terminal);
        assert!(replay.results().is_empty());
        assert!(
            !replay
                .engine()
                .is_fresh_press(&bound(u64::MAX, 6, 7, TouchPhase::Down))
        );
        replay.seek_cursor(0).unwrap();
        assert_eq!(replay.engine().stable_hash().unwrap(), pristine);
        assert_eq!(
            replay
                .engine()
                .is_fresh_press(&bound(u64::MAX, 0, 1, TouchPhase::Down)),
            tracks
        );
        replay.seek_cursor(1).unwrap();
        assert!(
            !replay
                .engine()
                .is_fresh_press(&bound(u64::MAX, 0, 1, TouchPhase::Down))
        );
        replay.seek_cursor(4).unwrap();
        assert_eq!(
            replay
                .engine()
                .is_fresh_press(&bound(u64::MAX, 4, 5, TouchPhase::Down)),
            tracks
        );
        replay.seek_cursor(7).unwrap();
        assert_eq!(replay.engine().stable_hash().unwrap(), terminal);
        if tracks {
            let mut erased = original.clone();
            erased.invisible.clear();
            assert!(
                validate_section_setup(&erased, &file, limits()).is_err(),
                "enabled empty contact identity cannot reconstruct as legacy disabled"
            );
        }
    }
}
