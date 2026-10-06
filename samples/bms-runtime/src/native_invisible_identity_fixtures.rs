//! Deferred native capture/compatibility fixtures. Typed preparation bypasses no
//! production admission guard; no native device or network is opened here.
use crate::{
    PreparedBms,
    competition::OpponentKind,
    competition_live::{CompetitionOptions, LiveCompetition},
    input_sounds::InputSoundIdentity,
    local_players::{PlayerId, ResolvedInputPlan},
    local_preparation::prepare_local_members,
    local_runtime::MemberConfig,
    multiplayer::competition_identity_for_section,
    native_cohort_setup::{CohortPreparation, prepare_cohort},
    native_group_competition::canonical_identity,
    native_judge::{prepare_capture, prepare_capture_for_source},
    replay_capture::LiveReplayCapture,
    replay_playback::{decode_section_setup, reconstruct},
    section_start::source_at,
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits, PcmSample, SampleBank, SampleId, VoiceId, command_queue},
    chart::Beat,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow},
    replay::codec::{ReplayCodecLimits, decode_replay},
    runtime::{Runtime, RuntimeProcessingClock, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsChart, BmsInputMode, parse};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const DOMAIN: ClockDomainId = ClockDomainId(17);
const START: i64 = 1_000_000_000;
const SEED: u64 = u64::MAX;
const TEXT: &str = "#BPM 60\n#WAV01 one\n#WAV02 two\n#00011:0001\n#00031:0102";
fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn source() -> BmsChart {
    parse(TEXT, Default::default()).unwrap()
}
fn limits(bytes: usize, header: usize) -> ReplayCodecLimits {
    ReplayCodecLimits::new(bytes, 32, header, CodecLimits::new(4096, 1024).unwrap()).unwrap()
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
fn judge(source: &BmsChart) -> JudgeEngine {
    JudgeEngine::new(source.compile().unwrap().chart, source.rules(), profile()).unwrap()
}
fn bindings(selector: DeviceSelector, key: u16) -> BindingMap {
    BindingMap::from_bindings([
        Binding {
            device: selector,
            physical: PhysicalControlId::keyboard(key),
            game_control: GameControlId(0x11),
        },
        Binding {
            device: selector,
            physical: PhysicalControlId::keyboard(key + 1),
            game_control: GameControlId(0x12),
        },
    ])
    .unwrap()
}
fn prepared(source: BmsChart) -> PreparedBms {
    let compiled = source.compile().unwrap();
    let sounds = source
        .notes
        .iter()
        .map(|note| SoundBinding {
            object: note.object,
            stage: JudgeStage::Instant,
            sample: note.sample,
            voice: VoiceId(note.object.0),
            gain: source.wav_gain().unwrap(),
        })
        .collect();
    let format = AudioFormat::new(10, 1).unwrap();
    let bounds = PcmLimits::new(64, 128, 4).unwrap();
    let mut bank = SampleBank::new(format, bounds).unwrap();
    for (id, pcm) in [(1, vec![0.5, -0.5]), (2, vec![0.25, -0.25])] {
        bank.insert(SampleId(id), PcmSample::new(format, pcm, bounds).unwrap())
            .unwrap();
    }
    PreparedBms {
        source,
        compiled,
        bank,
        sounds,
        bgm_commands: vec![],
    }
}
fn members(source: &BmsChart, count: usize) -> Vec<MemberConfig> {
    let prepared = prepared(source.clone());
    let plan = ResolvedInputPlan::new(
        (0..count)
            .map(|index| {
                (
                    PlayerId(u32::MAX - index as u32),
                    Some(DeviceId(u64::MAX - index as u64)),
                )
            })
            .collect(),
    )
    .unwrap();
    prepare_local_members(
        &prepared,
        &plan,
        (0..count)
            .map(|index| bindings(DeviceSelector::Exact(DeviceId(u64::MAX - index as u64)), 91))
            .collect(),
        profile(),
        BmsInputMode::ButtonOnly,
    )
    .unwrap()
    .configs
}
fn capture(source: &BmsChart, judge: &JudgeEngine) -> LiveReplayCapture {
    prepare_capture_for_source(
        source,
        judge,
        DOMAIN,
        ts(START),
        SEED,
        Some(limits(65536, 4096)),
    )
    .unwrap()
    .unwrap()
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
fn recorded(source: &BmsChart) -> (Vec<u8>, u64) {
    let prepared = prepared(source.clone());
    let judge = self::judge(source);
    let mut capture = capture(source, &judge);
    let (producer, _consumer) = command_queue(8).unwrap();
    let mut runtime = Runtime::new(
        DOMAIN,
        ClockDomainId(23),
        Transport::new(ts(0), ts(START), Rate::NORMAL),
        bindings(DeviceSelector::Any, 91),
        judge,
        producer,
        prepared.sounds,
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let input = PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(
            DeviceId(u64::MAX),
            ClockPoint {
                domain: DOMAIN,
                timestamp: ts(START),
            },
            u64::MAX,
        ),
        control: PhysicalControlId::keyboard(91),
        state: ButtonState::Down,
    });
    let report = runtime
        .process_input(
            input.clone(),
            &NoMapping,
            ClockPoint {
                domain: ClockDomainId(23),
                timestamp: ts(9_000_000_000),
            },
        )
        .unwrap();
    assert_eq!(report.song_time, ts(2_000_000_000));
    assert_eq!(report.bound_inputs.len(), 1);
    assert_eq!(report.bound_inputs[0].physical, input);
    assert_eq!(report.judge_events.len(), 1);
    assert!(report.judge_error.is_none() && report.audio_failures.is_empty());
    capture.record_report(&report).unwrap();
    let report = runtime
        .advance_to(
            ClockPoint {
                domain: DOMAIN,
                timestamp: ts(START + 1),
            },
            &NoMapping,
            ClockPoint {
                domain: ClockDomainId(23),
                timestamp: ts(9_000_000_001),
            },
        )
        .unwrap();
    capture.record_report(&report).unwrap();
    (
        capture.into_bytes().unwrap(),
        runtime.judge().stable_hash().unwrap(),
    )
}

#[test]
fn native_source_capture_preserves_legacy_bytes_and_bounds_without_mutating_the_pristine_judge() {
    let source = source_at(&source(), ts(START)).unwrap();
    let judge = self::judge(&source);
    let hash = judge.stable_hash().unwrap();
    let original = source.clone();
    let aware = capture(&source, &judge);
    let mut literal_identity = b"bms-judge-setup/v2:".to_vec();
    literal_identity.extend_from_slice(&hash.to_le_bytes());
    literal_identity.extend_from_slice(
        &InputSoundIdentity::from_source(&source)
            .unwrap()
            .unwrap()
            .fingerprint()
            .to_le_bytes(),
    );
    assert_eq!(aware.header().chart_identity, literal_identity);
    assert_eq!(
        aware.header().rules_identity,
        b"beatkernel-bms/builtin-judge/v1"
    );
    assert_eq!(aware.header().normalized_clock, DOMAIN);
    let setup = decode_section_setup(&aware.header().options).unwrap();
    assert_eq!(
        (setup.start, setup.chart_seed, setup.end, setup.input_mode),
        (ts(START), SEED, None, BmsInputMode::ButtonOnly)
    );
    let bytes = aware.into_bytes().unwrap();
    assert_eq!(
        prepare_capture_for_source(
            &source,
            &judge,
            DOMAIN,
            ts(START),
            SEED,
            Some(limits(bytes.len(), bytes.len()))
        )
        .unwrap()
        .unwrap()
        .into_bytes()
        .unwrap(),
        bytes
    );
    for bounds in [limits(bytes.len() - 1, bytes.len() - 1), limits(65536, 1)] {
        assert!(
            prepare_capture_for_source(&source, &judge, DOMAIN, ts(START), SEED, Some(bounds))
                .is_err()
        );
        assert_eq!(judge.stable_hash().unwrap(), hash);
    }
    let mut invalid = source.clone();
    invalid.invisible_ticks_per_beat = 0;
    assert!(
        prepare_capture_for_source(&invalid, &judge, DOMAIN, ts(-1), SEED, None)
            .unwrap()
            .is_none()
    );
    assert!(
        prepare_capture_for_source(
            &invalid,
            &judge,
            DOMAIN,
            ts(START),
            SEED,
            Some(limits(65536, 4096))
        )
        .is_err()
    );
    let mut started = self::judge(&source);
    started.advance_to(ts(START)).unwrap();
    let started_hash = started.stable_hash().unwrap();
    assert!(
        prepare_capture_for_source(
            &source,
            &started,
            DOMAIN,
            ts(START),
            SEED,
            Some(limits(65536, 4096))
        )
        .is_err()
    );
    assert_eq!(started.stable_hash().unwrap(), started_hash);
    assert_eq!(judge.stable_hash().unwrap(), hash);
    assert_eq!(source, original);

    let mut ordinary = source.clone();
    ordinary.invisible.clear();
    let judge = self::judge(&ordinary);
    for (start, seed) in [(0, 0), (START, 0), (START, SEED)] {
        let old = prepare_capture(&judge, DOMAIN, ts(start), seed, Some(limits(65536, 4096)))
            .unwrap()
            .unwrap()
            .into_bytes()
            .unwrap();
        let new = prepare_capture_for_source(
            &ordinary,
            &judge,
            DOMAIN,
            ts(start),
            seed,
            Some(limits(65536, 4096)),
        )
        .unwrap()
        .unwrap()
        .into_bytes()
        .unwrap();
        assert_eq!(new, old);
    }
    let (bytes, terminal) = recorded(&source);
    let file = decode_replay(&bytes, limits(65536, 4096)).unwrap();
    assert_eq!(file.records.len(), 2);
    let replay = reconstruct(&source, file, limits(65536, 4096)).unwrap();
    assert_eq!(replay.engine().stable_hash().unwrap(), terminal);
}

#[test]
fn actual_native_cohort_preparation_installs_identical_source_aware_captures_for_every_assigned_member()
 {
    let source = source_at(&source(), ts(START)).unwrap();
    let prepared = prepared(source.clone());
    let keys = BTreeMap::from([(0x11, 91_u16), (0x12, 92_u16)]);
    let cfg = CohortPreparation {
        host: DOMAIN,
        output: ClockDomainId(23),
        early: 0,
        late: 0,
        offset: 0,
        preroll: 100_000_000,
        start: ts(START),
        end: Some(ts(3_000_000_000)),
        chart_seed: SEED,
        bindings: &keys,
        record_replay: Some(Path::new("not-created/native.bkr")),
        replay_max_bytes: 65536,
        replay_max_records: 32,
    };
    let expected = crate::native_judge::prepare_section_capture_for_source(
        &source,
        &judge(&source),
        DOMAIN,
        ts(START),
        SEED,
        Some(ts(3_000_000_000)),
        Some(limits(65536, 4096)),
    )
    .unwrap()
    .unwrap()
    .header()
    .clone();
    for count in [2, 3, 4, 64] {
        let assignments: Vec<_> = (0..count)
            .map(|index| {
                (
                    PlayerId(u32::MAX - index as u32),
                    DeviceId(u64::MAX - index as u64),
                )
            })
            .collect();
        let cohort = prepare_cohort(
            &prepared,
            &assignments,
            &CompetitionOptions::default(),
            &cfg,
        )
        .unwrap();
        assert!(cohort.network.is_none());
        assert_eq!(cohort.states.len(), count);
        for ((member, state), &(player, device)) in
            cohort.configs.iter().zip(&cohort.states).zip(&assignments)
        {
            assert_eq!(
                (member.player, member.device, state.player),
                (player, Some(device), player)
            );
            assert_eq!(state.last_song, ts(900_000_000));
            assert!(state.completion.is_none() && state.competition.is_none());
            let captured = state.capture.as_ref().unwrap();
            assert_eq!(captured.header(), &expected);
            assert!(captured.records().is_empty());
            assert!(member.judge.effective_song_time().is_none());
            assert_eq!(
                decode_section_setup(&captured.header().options)
                    .unwrap()
                    .end,
                Some(ts(3_000_000_000)),
                "finite native capture retains its original-song endpoint"
            );
        }
        for (state, (_, path)) in cohort.states.into_iter().zip(cohort.save_paths) {
            assert_eq!(
                path.unwrap(),
                PathBuf::from(format!("not-created/native.p{}.bkr", state.player.0))
            );
            let file = state.capture.unwrap().into_file();
            crate::replay_playback::validate_section_setup(&source, &file, limits(65536, 4096))
                .unwrap();
        }
    }
    let mut disabled = cfg;
    disabled.record_replay = None;
    disabled.replay_max_bytes = 0;
    disabled.replay_max_records = 0;
    let cohort = prepare_cohort(
        &prepared,
        &[(PlayerId(7), DeviceId(3)), (PlayerId(9), DeviceId(4))],
        &CompetitionOptions::default(),
        &disabled,
    )
    .unwrap();
    assert!(cohort.states.iter().all(|s| s.capture.is_none()));
    assert!(cohort.save_paths.iter().all(|(_, p)| p.is_none()));
}

fn identity(
    source: &BmsChart,
    members: &[MemberConfig],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    canonical_identity(
        &CompetitionOptions::default(),
        source,
        members,
        DOMAIN,
        ts(START),
        SEED,
        Some(ts(3_000_000_000)),
        100_000_000,
    )
}
#[test]
fn pure_native_group_identity_ignores_local_routes_and_voices_but_rejects_source_or_judge_mismatch()
{
    let source = source_at(&source(), ts(START)).unwrap();
    let expected = competition_identity_for_section(
        capture(&source, &judge(&source)).header(),
        env!("CARGO_PKG_VERSION"),
        limits(65536, 4096),
        Some(ts(3_000_000_000)),
    )
    .unwrap();
    for count in [1, 2, 3, 64] {
        let mut members = self::members(&source, count);
        let hashes: Vec<_> = members
            .iter()
            .map(|m| m.judge.stable_hash().unwrap())
            .collect();
        assert_eq!(identity(&source, &members).unwrap(), expected);
        for (index, member) in members.iter_mut().enumerate() {
            member.player = PlayerId(index as u32 + 1);
            member.device = Some(DeviceId(index as u64 + 3));
            member.bindings = bindings(DeviceSelector::Exact(member.device.unwrap()), 400);
            for sound in &mut member.sounds {
                sound.voice = VoiceId(u64::MAX - index as u64);
            }
        }
        assert_eq!(identity(&source, &members).unwrap(), expected);
        assert_eq!(
            members
                .iter()
                .map(|m| m.judge.stable_hash().unwrap())
                .collect::<Vec<_>>(),
            hashes
        );
    }
    let mut variants = Vec::new();
    let mut changed = source.clone();
    changed.invisible[0].sample = SampleId(2);
    variants.push(changed);
    let mut changed = source.clone();
    changed.invisible[0].beat = Beat::new(1).unwrap();
    variants.push(changed);
    let mut changed = source.clone();
    changed.metadata.insert("VOLWAV".into(), "25".into());
    variants.push(changed);
    let mut changed = source.clone();
    changed.invisible[0].lane = parse("#WAV01 x\n#00032:01", Default::default())
        .unwrap()
        .invisible[0]
        .lane;
    variants.push(changed);
    for changed in variants {
        assert_eq!(
            changed.compile().unwrap().chart,
            source.compile().unwrap().chart
        );
        assert_ne!(identity(&changed, &members(&changed, 2)).unwrap(), expected);
    }
    let mut invalid = source.clone();
    invalid.invisible_ticks_per_beat = 0;
    assert!(identity(&invalid, &members(&source, 2)).is_err());
    let mut mismatched = members(&source, 2);
    let other = parse("#BPM 60\n#WAV01 one\n#00011:0100", Default::default()).unwrap();
    mismatched[1].judge = judge(&other);
    assert!(identity(&source, &mismatched).is_err());
    mismatched[0].judge = judge(&other);
    assert!(
        identity(&source, &mismatched).is_err(),
        "even equal members must match actual selected source"
    );
    let mut started = members(&source, 2);
    started[1].judge.advance_to(ts(START)).unwrap();
    assert!(identity(&source, &started).is_err());
    assert!(identity(&source, &[]).is_err());
    let mut duplicate = members(&source, 2);
    duplicate[1].player = duplicate[0].player;
    assert!(identity(&source, &duplicate).is_err());
}

struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "beatkernel-native-invisible-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn ghosts(path: PathBuf) -> CompetitionOptions {
    CompetitionOptions {
        ghosts: vec![(OpponentKind::Other, path)],
        ..CompetitionOptions::default()
    }
}
fn live(
    options: &CompetitionOptions,
    source: &BmsChart,
    judge: &JudgeEngine,
) -> Result<Option<LiveCompetition>, Box<dyn std::error::Error>> {
    LiveCompetition::prepare_for_at_with_chart_seed(
        PlayerId(7),
        options,
        source,
        judge,
        DOMAIN,
        ts(START),
        SEED,
    )
}

#[test]
fn actual_native_ghost_loading_accepts_recorded_identity_and_refuses_changes_before_unrelated_file_io()
 {
    let files = Files::new();
    let source = source_at(&source(), ts(START)).unwrap();
    let judge = self::judge(&source);
    let before = judge.stable_hash().unwrap();
    let (bytes, terminal) = recorded(&source);
    let file = decode_replay(&bytes, limits(65536, 4096)).unwrap();
    assert_eq!(
        reconstruct(&source, file, limits(65536, 4096))
            .unwrap()
            .engine()
            .stable_hash()
            .unwrap(),
        terminal
    );
    let options = ghosts(files.write("actual.bkr", &bytes));
    let mut owner = live(&options, &source, &judge).unwrap().unwrap();
    owner.finish();
    assert_eq!(judge.stable_hash().unwrap(), before);
    for modify in [
        (|s: &mut BmsChart| s.invisible[0].sample = SampleId(2)) as fn(&mut BmsChart),
        |s| s.invisible[0].beat = Beat::new(1).unwrap(),
        |s| {
            s.metadata.insert("VOLWAV".into(), "50".into());
        },
    ] {
        let mut changed = source.clone();
        modify(&mut changed);
        assert!(live(&options, &changed, &self::judge(&changed)).is_err());
    }
    let legacy = prepare_capture(&judge, DOMAIN, ts(START), SEED, Some(limits(65536, 4096)))
        .unwrap()
        .unwrap()
        .into_bytes()
        .unwrap();
    assert!(live(&ghosts(files.write("legacy.bkr", &legacy)), &source, &judge).is_err());
    let mut ordinary = source.clone();
    ordinary.invisible.clear();
    let (ordinary_bytes, _) = recorded(&ordinary);
    let mut legacy_owner = live(
        &ghosts(files.write("ordinary.bkr", &ordinary_bytes)),
        &ordinary,
        &self::judge(&ordinary),
    )
    .unwrap()
    .unwrap();
    legacy_owner.finish();

    let absent = ghosts(files.0.join("not-created.bkr"));
    let mut invalid = source.clone();
    invalid.invisible_ticks_per_beat = 0;
    let expected = InputSoundIdentity::from_source(&invalid).unwrap_err();
    let error = match live(&absent, &invalid, &judge) {
        Err(e) => e,
        Ok(_) => panic!("invalid source"),
    };
    assert_eq!(error.to_string(), expected);
    assert!(error.downcast_ref::<std::io::Error>().is_none());
    let error = match live(&absent, &source, &judge) {
        Err(e) => e,
        Ok(_) => panic!("missing actual file"),
    };
    assert_eq!(
        error.downcast_ref::<std::io::Error>().unwrap().kind(),
        std::io::ErrorKind::NotFound
    );
    assert!(
        live(&CompetitionOptions::default(), &invalid, &judge)
            .unwrap()
            .is_none()
    );
    let wrong = parse("#BPM 60\n#WAV01 one\n#00011:0100", Default::default()).unwrap();
    assert!(live(&options, &source, &self::judge(&wrong)).is_err());
    assert_eq!(judge.stable_hash().unwrap(), before);
}
