//! Deferred semantic identity and actual capture/reconstruction fixtures, without asset IO.
use crate::{
    PreparedBms,
    input_sounds::{InputSoundIdentity, InputSoundPlan},
    local_players::{PlayerId, ResolvedInputPlan},
    local_runtime::InputResult,
    replay_capture::{LiveReplayCapture, setup_input_header, setup_input_sound_header},
    replay_playback::{decode_section_setup, reconstruct_section, validate_section_setup},
    section_start::{prepare_at, source_at},
    step_gameplay::{StepGameplay, StepGameplayConfig, StepLocalGameplay},
};
use beatkernel::{
    audio::{AudioCommand, AudioFormat, PcmLimits, PcmSample, SampleBank, SampleId, VoiceId},
    chart::Beat,
    input::{
        Binding, BindingMap, CodecLimits, ContactId, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent, Position2, TouchEvent, TouchPhase,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow},
    replay::{
        ReplayHeader, ReplayOperation,
        codec::{ReplayCodecLimits, ReplayFile, decode_replay, encode_replay},
    },
    runtime::SoundBinding,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::{BmsChart, BmsInputMode, parse};

const CONTACT: BmsInputMode = BmsInputMode::ButtonOrContact;
const BUTTON: BmsInputMode = BmsInputMode::ButtonOnly;
const PRACTICE: &str = "#BPM 60\n#WAV01 key.wav\n#00011:0001\n#00031:0101";
fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn point(domain: u32, value: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(value),
    }
}
fn limits(header: usize) -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 64, header, CodecLimits::new(4096, 1024).unwrap()).unwrap()
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
fn source() -> BmsChart {
    parse(
        "#BPM 120\n#BPM01 240\n#STOP01 48\n#WAV0A upper\n#WAV0a lower\n#WAVzz last\n\
        #VOLWAV 25\n#00002:0.75\n#00008:000100\n#00009:000100\n\
        #00032:0a0000\n#00031:0A00zz\n#00131:0a\n#BASE 62",
        Default::default(),
    )
    .unwrap()
}
fn identity(source: &BmsChart) -> InputSoundIdentity {
    InputSoundIdentity::from_source(source).unwrap().unwrap()
}
fn judge(source: &BmsChart, mode: BmsInputMode) -> JudgeEngine {
    let constructor = if mode == CONTACT && !source.invisible.is_empty() {
        JudgeEngine::new_with_contacts
    } else {
        JudgeEngine::new
    };
    constructor(
        source.compile().unwrap().chart,
        source.rules_with_input_mode(mode),
        profile(),
    )
    .unwrap()
}
fn aware_header(
    judge: &JudgeEngine,
    start: i64,
    value: Option<InputSoundIdentity>,
) -> ReplayHeader {
    setup_input_sound_header(
        judge,
        ClockDomainId(1),
        limits(4096),
        ts(start),
        u64::MAX,
        None,
        CONTACT,
        value,
    )
    .unwrap()
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
fn bindings(selector: DeviceSelector) -> BindingMap {
    BindingMap::from_bindings([Binding {
        device: selector,
        physical: PhysicalControlId::keyboard(91),
        game_control: GameControlId(0x11),
    }])
    .unwrap()
}
fn input(source: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(DeviceId(source), point(1, 1_000_000_000), u64::MAX),
        control: PhysicalControlId::keyboard(91),
        contact: ContactId(u64::MAX),
        phase: TouchPhase::Down,
        position: Position2 { x: -4.0, y: 800.0 },
        pressure: Some(0.5),
    })
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
fn prepared(start: i64) -> PreparedBms {
    // Explicit typed composition keeps the unsupported prepare_from_source
    // admission guard outside this identity-only test. No decoder is called.
    let source = parse(PRACTICE, Default::default()).unwrap();
    let compiled = source.compile().unwrap();
    let format = AudioFormat::new(8000, 1).unwrap();
    let cap = PcmLimits::new(64, 128, 4).unwrap();
    let mut bank = SampleBank::new(format, cap).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, -0.25], cap).unwrap(),
    )
    .unwrap();
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
    prepare_at(
        PreparedBms {
            source,
            compiled,
            bank,
            sounds,
            bgm_commands: vec![],
        },
        ts(start),
        cap,
    )
    .unwrap()
    .0
}

#[test]
fn identity_covers_semantic_count_time_control_sample_and_gain_but_not_source_provenance_or_voices()
{
    let source = source();
    let before = source.clone();
    let expected = identity(&source);
    let mut semantic: Vec<_> = source
        .compile_invisible()
        .unwrap()
        .iter()
        .map(|entry| (entry.lane.control().0, entry.at.as_nanos(), entry.sample.0))
        .collect();
    semantic.sort();
    assert_eq!(
        semantic,
        [
            (0x11, 0, 10),
            (0x11, 1_000_000_000, 3843),
            (0x11, 1_250_000_000, 36),
            (0x12, 0, 36)
        ]
    );
    assert_eq!(source.wav_gain().unwrap().to_bits(), 0x3e80_0000);
    let reordered = parse("; extra physical line\n#BASE 62\n#WAVzz changed-last\n#WAV0a changed-lower\n#WAV0A changed-upper\n\
        #TITLE descriptive-only\n#VOLWAV 25\n#BPM 120\n#BPM01 240\n#STOP01 48\n\
        #00002:0.75\n#00008:000100\n#00009:000100\n#00131:0a\n#00031:0A00zz\n#00032:0a0000", Default::default()).unwrap();
    assert_eq!(identity(&reordered), expected);
    let mut provenance = source.clone();
    provenance.invisible.reverse();
    for (index, event) in provenance.invisible.iter_mut().enumerate() {
        event.ordinal = u64::MAX - index as u64;
        event.line = 500 + index;
    }
    assert_eq!(identity(&provenance), expected);
    let lane13 = parse("#WAV01 x\n#00033:01", Default::default())
        .unwrap()
        .invisible[0]
        .lane;
    let mut count = source.clone();
    count.invisible.pop();
    let mut time = source.clone();
    let first = time
        .invisible
        .iter_mut()
        .find(|entry| entry.lane.channel() == 0x11 && entry.beat.ticks() == 0)
        .unwrap();
    first.beat = Beat::new(1).unwrap();
    let mut control = source.clone();
    control
        .invisible
        .iter_mut()
        .find(|entry| entry.lane.channel() == 0x12)
        .unwrap()
        .lane = lane13;
    let mut sample = source.clone();
    sample.invisible[0].sample = SampleId(10);
    let mut gain = source.clone();
    gain.metadata.insert("VOLWAV".into(), "50".into());
    for changed in [count, time, control, sample, gain] {
        assert_ne!(identity(&changed), expected);
    }
    let ordinary = InputSoundPlan::prepare(&source, &[], &[], 4).unwrap();
    let occupied = [SoundBinding {
        object: beatkernel::chart::ObjectId(9),
        stage: JudgeStage::Instant,
        sample: SampleId(10),
        voice: VoiceId(9_007_199_254_740_993),
        gain: 1.0,
    }];
    let remapped = InputSoundPlan::prepare(&source, &occupied, &[], 4).unwrap();
    assert_ne!(ordinary.markers()[0].voice, remapped.markers()[0].voice);
    assert_eq!(identity(&source), expected);
    assert_eq!(source, before);
    assert_eq!(
        InputSoundIdentity::from_source(&parse("#BPM 120", Default::default()).unwrap()).unwrap(),
        None
    );
    let mut duplicate = source.clone();
    duplicate.invisible.push(duplicate.invisible[0]);
    let mut bad_grid = source.clone();
    bad_grid.invisible_ticks_per_beat = 0;
    let mut missing = source.clone();
    missing.invisible[0].sample = SampleId(3844);
    let mut bad_gain = source.clone();
    bad_gain.metadata.insert("VOLWAV".into(), "NaN".into());
    for invalid in [duplicate, bad_grid, missing, bad_gain] {
        let before = invalid.clone();
        assert!(InputSoundIdentity::from_source(&invalid).is_err());
        assert_eq!(invalid, before);
    }
}

#[test]
fn aware_header_has_literal_v2_layout_while_none_keeps_every_legacy_mode_and_capacity_boundary() {
    let source = source();
    let identity = identity(&source);
    let judge = judge(&source, CONTACT);
    let old = setup_input_header(
        &judge,
        ClockDomainId(1),
        limits(4096),
        ts(0),
        0,
        None,
        CONTACT,
    )
    .unwrap();
    let aware = setup_input_sound_header(
        &judge,
        ClockDomainId(1),
        limits(4096),
        ts(0),
        0,
        None,
        CONTACT,
        Some(identity),
    )
    .unwrap();
    let mut expected = b"bms-judge-setup/v2:".to_vec();
    expected.extend_from_slice(&judge.stable_hash().unwrap().to_le_bytes());
    expected.extend_from_slice(&identity.fingerprint().to_le_bytes());
    assert_eq!(aware.chart_identity, expected);
    assert_eq!(aware.chart_identity.len(), 35);
    assert_eq!(aware.rules_identity, b"beatkernel-bms/press-judge/v1");
    assert_eq!(aware.options, old.options);
    assert_eq!(aware.version, old.version);
    assert_eq!(aware.normalized_clock, old.normalized_clock);
    assert_eq!(aware.seed, old.seed);
    let mut options = b"bms-judge-profile/v5:".to_vec();
    options.push(1);
    options.extend_from_slice(&0u64.to_le_bytes());
    options.extend_from_slice(&0i64.to_le_bytes());
    options.push(0);
    options.extend_from_slice(&0i64.to_le_bytes());
    options.extend_from_slice(&1u64.to_le_bytes());
    options.extend_from_slice(&1u32.to_le_bytes());
    options.extend_from_slice(&0i64.to_le_bytes());
    options.extend_from_slice(&0i64.to_le_bytes());
    assert_eq!(aware.options, options);
    let exact_old = old.chart_identity.len()
        + old.options.len()
        + old.rules_identity.len()
        + env!("CARGO_PKG_VERSION").len();
    assert!(
        setup_input_sound_header(
            &judge,
            ClockDomainId(1),
            limits(exact_old),
            ts(0),
            0,
            None,
            CONTACT,
            None
        )
        .is_ok()
    );
    assert!(
        setup_input_sound_header(
            &judge,
            ClockDomainId(1),
            limits(exact_old + 7),
            ts(0),
            0,
            None,
            CONTACT,
            Some(identity)
        )
        .is_err()
    );
    assert_eq!(
        setup_input_sound_header(
            &judge,
            ClockDomainId(1),
            limits(exact_old + 8),
            ts(0),
            0,
            None,
            CONTACT,
            Some(identity)
        )
        .unwrap(),
        aware
    );
    let wire = encode_replay(&ReplayFile::new(aware.clone(), vec![]), limits(4096)).unwrap();
    let cap = ReplayCodecLimits::new(
        wire.len() - 1,
        64,
        exact_old + 8,
        CodecLimits::new(4096, 1024).unwrap(),
    )
    .unwrap();
    let hash = judge.stable_hash().unwrap();
    assert!(
        LiveReplayCapture::new_with_input_sounds(
            &judge,
            ClockDomainId(1),
            cap,
            ts(0),
            0,
            None,
            CONTACT,
            Some(identity)
        )
        .is_err()
    );
    assert_eq!(judge.stable_hash().unwrap(), hash);
    assert_eq!(
        LiveReplayCapture::new_with_input_sounds(
            &judge,
            ClockDomainId(1),
            limits(4096),
            ts(0),
            0,
            None,
            CONTACT,
            Some(identity)
        )
        .unwrap()
        .into_bytes()
        .unwrap(),
        wire
    );
    let empty = parse("#BPM 120", Default::default()).unwrap();
    for (mode, start, seed, end) in [
        (BUTTON, 0, 0, None),
        (BUTTON, 1, 0, None),
        (BUTTON, 1, u64::MAX, None),
        (BUTTON, 1, 0, Some(ts(9))),
        (CONTACT, 1, u64::MAX, Some(ts(9))),
    ] {
        let judge = self::judge(&empty, mode);
        let old = setup_input_header(
            &judge,
            ClockDomainId(1),
            limits(4096),
            ts(start),
            seed,
            end,
            mode,
        )
        .unwrap();
        let new = setup_input_sound_header(
            &judge,
            ClockDomainId(1),
            limits(4096),
            ts(start),
            seed,
            end,
            mode,
            None,
        )
        .unwrap();
        assert_eq!(new, old);
        let legacy = LiveReplayCapture::new_with_input_mode(
            &judge,
            ClockDomainId(1),
            limits(4096),
            ts(start),
            seed,
            end,
            mode,
        )
        .unwrap()
        .into_bytes()
        .unwrap();
        let aware = LiveReplayCapture::new_with_input_sounds(
            &judge,
            ClockDomainId(1),
            limits(4096),
            ts(start),
            seed,
            end,
            mode,
            None,
        )
        .unwrap()
        .into_bytes()
        .unwrap();
        assert_eq!(aware, legacy);
    }
}

#[test]
fn actual_solo_and_local_capture_share_source_identity_and_reconstruct_practice_with_past_selections()
 {
    let original = parse(PRACTICE, Default::default()).unwrap();
    let selected = source_at(&original, ts(1_000_000_000)).unwrap();
    assert_eq!(selected.invisible.len(), 2);
    assert_eq!(identity(&selected), identity(&original));
    assert_eq!(
        InputSoundPlan::prepare(&selected, &[], &[], 2)
            .unwrap()
            .markers()[0]
            .at,
        ts(0)
    );
    let cap = PcmLimits::new(64, 128, 4).unwrap();
    let (only_invisible, selected_report) =
        prepare_at(prepared(0), ts(3_000_000_000), cap).unwrap();
    assert_eq!(selected_report.excluded_objects, 1);
    assert!(
        only_invisible.compiled.chart.objects().is_empty()
            && only_invisible.sounds.is_empty()
            && only_invisible.bgm_commands.is_empty()
    );
    assert_eq!(only_invisible.source.invisible.len(), 2);
    assert_eq!(identity(&only_invisible.source), identity(&original));
    let mut ordinary = prepared(0);
    ordinary.source.invisible.clear();
    assert!(
        prepare_at(ordinary, ts(3_000_000_000), cap).is_err(),
        "ordinary empty practice sections remain rejected"
    );
    // Real BGM suffix allocation reserves the high invisible PCM identity even
    // though it appears in neither gameplay sounds nor the original BGM cue.
    let source = parse(
        "#BASE 62\n#BPM 60\n#WAV01 music\n#WAVzz fallback\n#00001:01\n#00031:zz",
        Default::default(),
    )
    .unwrap();
    let sound_identity = identity(&source);
    let compiled = source.compile().unwrap();
    assert!(compiled.chart.objects().is_empty());
    assert_eq!(compiled.bgm[0].at, ts(0));
    let format = AudioFormat::new(8000, 1).unwrap();
    let mut bank = SampleBank::new(format, cap).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, -0.25], cap).unwrap(),
    )
    .unwrap();
    bank.insert(
        SampleId(3843),
        PcmSample::new(format, vec![0.5, -0.5], cap).unwrap(),
    )
    .unwrap();
    let cue = beatkernel::audio::AudioCommand::Play {
        sample: compiled.bgm[0].sample,
        voice: VoiceId(7),
        at: compiled.bgm[0].at,
        gain: 1.0,
    };
    let (tail, report) = prepare_at(
        PreparedBms {
            source,
            compiled,
            bank,
            sounds: vec![],
            bgm_commands: vec![cue],
        },
        ts(125_000),
        cap,
    )
    .unwrap();
    assert_eq!(report.tails.len(), 1);
    assert_eq!(report.tails[0].source, SampleId(1));
    assert_eq!(report.tails[0].suffix, SampleId(3844));
    assert_eq!(report.tails[0].frame, 1);
    assert_eq!(tail.bank.len(), 3);
    assert!(tail.bank.get(SampleId(2)).is_none());
    assert_eq!(
        tail.bank.get(SampleId(3843)).unwrap().samples(),
        [0.5, -0.5]
    );
    assert_eq!(tail.bank.get(SampleId(3844)).unwrap().samples(), [-0.25]);
    assert_eq!(
        tail.bgm_commands,
        [beatkernel::audio::AudioCommand::Play {
            sample: SampleId(3844),
            voice: VoiceId(7),
            at: ts(125_000),
            gain: 1.0
        }]
    );
    assert_eq!(identity(&tail.source), sound_identity);
    let (mut solo, _bank) = StepGameplay::new_section_with_input_mode(
        prepared(1_000_000_000),
        config(),
        bindings(DeviceSelector::Any),
        ts(1_000_000_000),
        None,
        CONTACT,
    )
    .unwrap();
    let header = solo.competition_header(limits(4096), u64::MAX).unwrap();
    assert_eq!(
        header,
        aware_header(solo.judge(), 1_000_000_000, Some(identity(&original)))
    );
    assert!(solo.configure_capture(limits(1), u64::MAX).is_err());
    assert!(!solo.failed());
    assert_eq!(
        solo.competition_header(limits(4096), u64::MAX).unwrap(),
        header
    );
    solo.configure_capture(limits(4096), u64::MAX).unwrap();
    solo.activate(config().host_origin).unwrap();
    let event = input(u64::MAX);
    let report = solo
        .process_input(event.clone(), &NoMapping, point(2, 9_000_000_000))
        .unwrap();
    assert!(report.judge_events.is_empty() && report.audio_failures.is_empty());
    assert_eq!(
        report.audio_commands,
        [AudioCommand::Play {
            sample: SampleId(1),
            voice: VoiceId(2),
            at: ts(9_000_000_000),
            gain: 1.0,
        }]
    );
    let terminal = solo.judge().stable_hash().unwrap();
    solo.fail();
    let file = decode_replay(&solo.take_replay().unwrap().unwrap(), limits(4096)).unwrap();
    assert_eq!(file.header, header);
    assert_eq!(file.records.len(), 1);
    assert_eq!(file.records[0].song_time, ts(1_000_000_000));
    assert_eq!(
        file.records[0].operation,
        ReplayOperation::Input(report.bound_inputs[0].clone())
    );
    let setup = decode_section_setup(&file.header.options).unwrap();
    assert_eq!(setup.start, ts(1_000_000_000));
    assert_eq!(
        reconstruct_section(&original, file, limits(4096))
            .unwrap()
            .engine()
            .stable_hash()
            .unwrap(),
        terminal
    );

    let players = [PlayerId(7), PlayerId(u32::MAX), PlayerId(91)];
    let devices = [DeviceId(3), DeviceId(u64::MAX), DeviceId(4)];
    let plan = ResolvedInputPlan::new(
        players
            .into_iter()
            .zip(devices.into_iter().map(Some))
            .collect(),
    )
    .unwrap();
    let (mut local, _bank) = StepLocalGameplay::new_section(
        prepared(1_000_000_000),
        config(),
        plan,
        devices
            .into_iter()
            .map(|device| bindings(DeviceSelector::Exact(device)))
            .collect(),
        ts(1_000_000_000),
        None,
        CONTACT,
    )
    .unwrap();
    for player in players {
        assert_eq!(
            local
                .competition_header(player, limits(4096), u64::MAX)
                .unwrap(),
            header,
            "member voice/device assignments are outside semantic input-sound identity"
        );
        local
            .configure_capture(player, limits(4096), u64::MAX)
            .unwrap();
    }
    local.activate(config().host_origin).unwrap();
    for ((player, device), voice) in players.into_iter().zip(devices).zip([4, 5, 6]) {
        let InputResult::Processed(reports) = local
            .process_input(input(device.0), &NoMapping, point(2, 9_000_000_000))
            .unwrap()
        else {
            panic!("assigned local source must be processed")
        };
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].player, player);
        assert!(reports[0].report.judge_events.is_empty());
        assert!(reports[0].report.audio_failures.is_empty());
        assert_eq!(
            reports[0].report.audio_commands,
            [AudioCommand::Play {
                sample: SampleId(1),
                voice: VoiceId(voice),
                at: ts(9_000_000_000),
                gain: 1.0,
            }]
        );
    }
    local.fail();
    for (player, device) in players.into_iter().zip(devices) {
        let file =
            decode_replay(&local.take_replay(player).unwrap().unwrap(), limits(4096)).unwrap();
        assert_eq!(file.header, header);
        assert_eq!(file.records.len(), 1);
        let ReplayOperation::Input(input) = &file.records[0].operation else {
            panic!("actual member contact must remain input")
        };
        assert_eq!(input.physical.meta().source, device);
        let replay = reconstruct_section(&original, file, limits(4096)).unwrap();
        assert_eq!(
            replay.engine().stable_hash().unwrap(),
            local.judge(player).unwrap().stable_hash().unwrap()
        );
        assert!(local.take_replay(player).unwrap().is_none());
    }
}

#[test]
fn replay_refuses_missing_injected_changed_or_malformed_identity_before_any_recorded_operation() {
    let original = parse(PRACTICE, Default::default()).unwrap();
    let selected = source_at(&original, ts(1_000_000_000)).unwrap();
    let judge = judge(&selected, CONTACT);
    let header = aware_header(&judge, 1_000_000_000, Some(identity(&selected)));
    let file = ReplayFile::new(header.clone(), vec![]);
    let before = file.clone();
    assert!(
        validate_section_setup(&original, &file, limits(4096))
            .unwrap()
            .effective_song_time()
            .is_none()
    );
    let mut past_removed = original.clone();
    past_removed.invisible.remove(0);
    let mut all_removed = original.clone();
    all_removed.invisible.clear();
    let mut gain = original.clone();
    gain.metadata.insert("VOLWAV".into(), "50".into());
    let mut sample = original.clone();
    sample.samples.insert(2, "other.wav".into());
    sample.invisible[0].sample = SampleId(2);
    let mut time = original.clone();
    time.invisible[0].beat = Beat::new(1).unwrap();
    for wrong in [past_removed, all_removed, gain, sample, time] {
        assert!(validate_section_setup(&wrong, &file, limits(4096)).is_err());
        assert!(reconstruct_section(&wrong, file.clone(), limits(4096)).is_err());
    }
    let legacy = setup_input_header(
        &judge,
        ClockDomainId(1),
        limits(4096),
        ts(1_000_000_000),
        u64::MAX,
        None,
        CONTACT,
    )
    .unwrap();
    assert!(
        validate_section_setup(&original, &ReplayFile::new(legacy, vec![]), limits(4096)).is_err()
    );
    let mut malformed = Vec::new();
    let mut short = header.clone();
    short.chart_identity.pop();
    malformed.push(short);
    let mut long = header.clone();
    long.chart_identity.push(0);
    malformed.push(long);
    let mut wrong_version = header.clone();
    wrong_version.chart_identity[b"bms-judge-setup/v".len()] = b'9';
    malformed.push(wrong_version);
    let mut wrong_fingerprint = header.clone();
    *wrong_fingerprint.chart_identity.last_mut().unwrap() ^= 1;
    malformed.push(wrong_fingerprint);
    let mut wrong_judge = header.clone();
    wrong_judge.chart_identity[b"bms-judge-setup/v2:".len()] ^= 1;
    malformed.push(wrong_judge);
    for header in malformed {
        assert!(
            validate_section_setup(&original, &ReplayFile::new(header, vec![]), limits(4096))
                .is_err()
        );
    }
    let mut clean = original.clone();
    clean.invisible.clear();
    let clean_selected = source_at(&clean, ts(1_000_000_000)).unwrap();
    let clean_judge = self::judge(&clean_selected, CONTACT);
    let clean_header = aware_header(&clean_judge, 1_000_000_000, None);
    let clean_file = ReplayFile::new(clean_header, vec![]);
    assert!(validate_section_setup(&clean, &clean_file, limits(4096)).is_ok());
    assert!(validate_section_setup(&original, &clean_file, limits(4096)).is_err());
    let injected = aware_header(&clean_judge, 1_000_000_000, Some(identity(&original)));
    assert!(
        validate_section_setup(&clean, &ReplayFile::new(injected, vec![]), limits(4096)).is_err()
    );
    assert_eq!(file, before);
    assert!(validate_section_setup(&original, &file, limits(4096)).is_ok());
}
