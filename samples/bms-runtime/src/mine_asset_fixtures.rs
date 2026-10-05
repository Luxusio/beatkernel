//! Deferred guarded asset preparation with real scoped files and WAV decoding.
//! These tests neither authorize mine-file playback nor open native resources.
use crate::{
    AssetDecoder, ChannelPolicy, PreparedBms, WavDecoder,
    asset_paths::AssetPathPolicy,
    asset_source::{AssetSource, MemoryAssetLimits, MemoryFiles},
    audio_assets::{load_bank, referenced_samples},
    native_audio::prepare_mine_sounds,
    native_judge::NativeJudgeConfig,
    prepare_from_source,
    replay_playback::PlaybackError,
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample,
        SampleBank, SampleId, VoiceId, command_queue,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{HazardOutcome, JudgeStage},
    replay::{
        ReplayHeader,
        codec::{ReplayCodecLimits, ReplayFile},
    },
    runtime::{Runtime, RuntimeProcessingClock, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsChart, BmsError, MineDamage, parse};
use std::{
    borrow::Cow,
    cell::RefCell,
    collections::BTreeSet,
    error::Error,
    io,
    path::{Path, PathBuf},
};

const CHART: &str = "pack/chart.bms";
const UNION: &str = "#BPM 60\n#WAV00 blast.wav\n#WAV01 note.wav\n#WAV02 bgm.wav\n#WAV03 unused.wav\n#WAV0A alias.wav\n#WAV0a lower.wav\n#WAVzz last.wav\n#00011:01\n#00001:02\n#00031:0A0azz\n#00032:000100\n#000D1:1EZZ\n#BASE 62";
fn format(channels: u16) -> AudioFormat {
    AudioFormat::new(10, channels).unwrap()
}
fn bounds() -> PcmLimits {
    PcmLimits::new(64, 256, 8).unwrap()
}
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(ns),
    }
}
fn source(text: &str) -> BmsChart {
    parse(text, Default::default()).unwrap()
}
fn references(source: &BmsChart, cap: usize) -> Result<BTreeSet<SampleId>, Box<dyn Error>> {
    referenced_samples(
        source,
        &source.compile()?,
        &source.compile_invisible()?,
        cap,
    )
}
fn ids(values: &[u64]) -> BTreeSet<SampleId> {
    values.iter().copied().map(SampleId).collect()
}
fn wav(rate: u32, channels: u16, samples: &[f32]) -> Vec<u8> {
    let bytes = (samples.len() * 4) as u32;
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(36 + bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(channels) * 4).to_le_bytes());
    out.extend_from_slice(&(channels * 4).to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&bytes.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}
struct Assets {
    files: MemoryFiles,
    resolved: RefCell<Vec<(String, AssetPathPolicy)>>,
    reads: RefCell<Vec<(PathBuf, usize)>>,
    read_error: Option<io::ErrorKind>,
}
impl Assets {
    fn new(text: &str, files: &[(&str, Vec<u8>)]) -> Self {
        let mut selected = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
        selected.insert(CHART, text.as_bytes().to_vec()).unwrap();
        for (name, bytes) in files {
            selected
                .insert(&format!("pack/{name}"), bytes.clone())
                .unwrap();
        }
        Self {
            files: selected,
            resolved: RefCell::new(Vec::new()),
            reads: RefCell::new(Vec::new()),
            read_error: None,
        }
    }
}
impl AssetSource for Assets {
    fn resolve(&self, name: &str, policy: AssetPathPolicy) -> io::Result<PathBuf> {
        self.resolved.borrow_mut().push((name.into(), policy));
        self.files.scope(CHART)?.resolve(name, policy)
    }
    fn read<'a>(&'a self, path: &Path, max_bytes: usize) -> io::Result<Cow<'a, [u8]>> {
        self.reads.borrow_mut().push((path.into(), max_bytes));
        if let Some(kind) = self.read_error {
            return Err(io::Error::new(
                kind,
                "controlled bounded source read refusal",
            ));
        }
        Ok(Cow::Owned(
            self.files.scope(CHART)?.read(path, max_bytes)?.into_owned(),
        ))
    }
}
#[derive(Default)]
struct Decoder {
    calls: RefCell<Vec<PathBuf>>,
    ignore_bound: bool,
}
impl AssetDecoder for Decoder {
    fn decode(
        &self,
        path: &Path,
        bytes: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>> {
        self.calls.borrow_mut().push(path.into());
        if self.ignore_bound {
            // A custom decoder may disregard its contract; bank insertion must recheck.
            return Ok(PcmSample::new(format(1), vec![0.5; 4], bounds())?);
        }
        WavDecoder.decode(path, bytes, limits)
    }
}
fn bank(
    source: &BmsChart,
    assets: &Assets,
    decoder: &Decoder,
) -> Result<SampleBank, Box<dyn Error>> {
    load_bank(
        source,
        &references(source, 8)?,
        assets,
        format(1),
        bounds(),
        ChannelPolicy::Exact,
        decoder,
        AssetPathPolicy::Exact,
    )
}
fn no_io(assets: &Assets, decoder: &Decoder) {
    assert!(assets.resolved.borrow().is_empty());
    assert!(assets.reads.borrow().is_empty());
    assert!(decoder.calls.borrow().is_empty());
}

#[test]
fn selection_unions_original_ids_and_optional_zero_with_literal_radix_and_pre_io_capacity() {
    let original = source(UNION);
    let before = original.clone();
    assert_eq!(
        references(&original, 6).unwrap(),
        ids(&[0, 1, 2, 10, 36, 3843])
    );
    assert!(references(&original, 5).is_err());
    assert_eq!(original, before);
    let mut absent = original.clone();
    absent.samples.remove(&0);
    assert_eq!(references(&absent, 5).unwrap(), ids(&[1, 2, 10, 36, 3843]));
    let mut fatal = original.clone();
    for mine in &mut fatal.mines {
        mine.damage = MineDamage::from_raw(1295).unwrap();
    }
    assert_eq!(references(&fatal, 5).unwrap(), ids(&[1, 2, 10, 36, 3843]));
    for text in [
        "#BPM 60\n#WAV00 unused.wav",
        "#BPM 60\n#WAV00 unused.wav\n#000D1:00000000",
        "#BPM 60\n#SETRANDOM 1\n#IF 2\n#BASE invalid\n#WAV00 unused.wav\n#000D1:invalid\n#ENDIF",
    ] {
        assert!(references(&source(text), 0).unwrap().is_empty());
    }
    for mut invalid in [original.clone(), absent, fatal] {
        invalid.mine_ticks_per_beat = 0;
        assert!(
            references(&invalid, 6).is_err(),
            "silent mine data still requires valid original timing"
        );
    }
    let mut duplicate = original;
    duplicate.mines[1].ordinal = duplicate.mines[0].ordinal;
    assert!(references(&duplicate, 6).is_err());
}

#[test]
fn scoped_literal_and_compatible_aliases_decode_once_and_keep_distinct_ungained_pcm_ids() {
    let text = "#BASE 62\n#BPM 60\n#VOLWAV 25\n#WAV00 音\\shared.ogg\n#WAV01 音/shared.WAV\n#WAV0A ./音/shared.WAV\n#00011:01\n#00031:0A\n#000D1:1E";
    let original = source(text);
    let before = original.clone();
    let assets = Assets::new(text, &[("音/shared.WAV", wav(10, 1, &[0.5, -0.25]))]);
    let exact = Decoder::default();
    assert!(bank(&original, &assets, &exact).is_err());
    assert!(assets.reads.borrow().is_empty() && exact.calls.borrow().is_empty());
    let assets = Assets::new(text, &[("音/shared.WAV", wav(10, 1, &[0.5, -0.25]))]);
    let decoder = Decoder::default();
    let loaded = load_bank(
        &original,
        &references(&original, 3).unwrap(),
        &assets,
        format(1),
        PcmLimits::new(8, 24, 3).unwrap(),
        ChannelPolicy::Exact,
        &decoder,
        AssetPathPolicy::AudioVariants,
    )
    .unwrap();
    assert_eq!((loaded.len(), loaded.total_bytes()), (3, 24));
    for id in [0, 1, 10] {
        assert_eq!(loaded.get(SampleId(id)).unwrap().samples(), [0.5, -0.25]);
    }
    let pointers = [0, 1, 10].map(|id| loaded.get(SampleId(id)).unwrap().samples().as_ptr());
    assert!(pointers[0] != pointers[1] && pointers[0] != pointers[2] && pointers[1] != pointers[2]);
    assert_eq!(
        *assets.resolved.borrow(),
        [
            ("音\\shared.ogg".into(), AssetPathPolicy::AudioVariants),
            ("音/shared.WAV".into(), AssetPathPolicy::AudioVariants),
            ("./音/shared.WAV".into(), AssetPathPolicy::AudioVariants)
        ]
    );
    assert_eq!(
        *assets.reads.borrow(),
        [(PathBuf::from("pack/音/shared.WAV"), 64 * 1024 * 1024)]
    );
    assert_eq!(
        *decoder.calls.borrow(),
        [PathBuf::from("pack/音/shared.WAV")]
    );
    assert_eq!(original, before);
    let literal = Assets::new(
        text,
        &[
            ("音/shared.ogg", wav(10, 1, &[-0.5, 0.75])),
            ("音/shared.WAV", wav(10, 1, &[0.5, -0.25])),
        ],
    );
    let decoder = Decoder::default();
    let loaded = load_bank(
        &original,
        &references(&original, 3).unwrap(),
        &literal,
        format(1),
        bounds(),
        ChannelPolicy::Exact,
        &decoder,
        AssetPathPolicy::AudioVariants,
    )
    .unwrap();
    assert_eq!(loaded.get(SampleId(0)).unwrap().samples(), [-0.5, 0.75]);
    assert_eq!(loaded.get(SampleId(1)).unwrap().samples(), [0.5, -0.25]);
    assert_eq!(
        *decoder.calls.borrow(),
        [
            PathBuf::from("pack/音/shared.ogg"),
            PathBuf::from("pack/音/shared.WAV")
        ]
    );
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
fn loaded_zero_pcm_reaches_actual_shared_mine_plan_runtime_queue_and_software_mixer() {
    let text =
        "#BPM 60\n#VOLWAV 50\n#WAV00 same.wav\n#WAV01 ./same.wav\n#00011:01\n#000D1:1E000100";
    let source = source(text);
    let compiled = source.compile().unwrap();
    let assets = Assets::new(text, &[("same.wav", wav(10, 1, &[0.5, -0.25]))]);
    let decoder = Decoder::default();
    let bank = bank(&source, &assets, &decoder).unwrap();
    assert_eq!(bank.len(), 2);
    assert_eq!(decoder.calls.borrow().len(), 1);
    let sounds = vec![SoundBinding {
        object: source.notes[0].object,
        stage: JudgeStage::Instant,
        sample: SampleId(1),
        voice: VoiceId(7),
        gain: source.wav_gain().unwrap(),
    }];
    let prepared = PreparedBms {
        source,
        compiled,
        bank,
        sounds,
        bgm_commands: Vec::new(),
    };
    let timeline = prepare_mine_sounds(&prepared, None).unwrap().unwrap();
    assert_eq!(
        timeline
            .bindings()
            .iter()
            .map(|binding| (binding.sample, binding.voice, binding.gain))
            .collect::<Vec<_>>(),
        [
            (SampleId(0), VoiceId(8), 0.5),
            (SampleId(0), VoiceId(8), 0.5)
        ]
    );
    let judge = NativeJudgeConfig {
        early: 0,
        late: 0,
        offset: 0,
        preroll: 0,
        output: ClockDomainId(2),
        end: None,
    }
    .judge(&prepared.source, prepared.compiled.chart.clone())
    .unwrap();
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(91u16),
        game_control: GameControlId(0x11),
    }])
    .unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(10_000_000_000), ts(0), Rate::NORMAL),
        bindings,
        judge,
        producer,
        prepared.sounds,
        0,
    )
    .unwrap();
    runtime.configure_hazard_sounds(timeline).unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let input = PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(u64::MAX), point(1, 10_000_000_000), u64::MAX),
        control: PhysicalControlId::keyboard(91u16),
        state: ButtonState::Down,
    });
    let first = runtime
        .process_input(input, &NoMapping, point(2, 1_000_000_000))
        .unwrap();
    assert_eq!(
        first.audio_commands,
        [
            AudioCommand::Play {
                sample: SampleId(1),
                voice: VoiceId(7),
                at: ts(1_000_000_000),
                gain: 0.5
            },
            AudioCommand::Play {
                sample: SampleId(0),
                voice: VoiceId(8),
                at: ts(1_000_000_000),
                gain: 0.5
            }
        ]
    );
    let held = runtime
        .advance_to(
            point(1, 12_000_000_000),
            &NoMapping,
            point(2, 3_000_000_000),
        )
        .unwrap();
    assert_eq!(held.hazard_events[0].outcome, HazardOutcome::Triggered);
    assert_eq!(
        held.audio_commands,
        [AudioCommand::Play {
            sample: SampleId(0),
            voice: VoiceId(8),
            at: ts(3_000_000_000),
            gain: 0.5
        }]
    );
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format(1),
            ClockDomainId(2),
            ts(1_000_000_000),
            AudioLimits::new(8, 4, 8, 32, 8).unwrap(),
        ),
        prepared.bank,
        consumer,
    )
    .unwrap();
    let mut pcm = [0.0; 24];
    let report = mixer.render(&mut pcm).unwrap();
    let mut expected = [0.0; 24];
    expected[0] = 0.5;
    expected[1] = -0.25;
    expected[20] = 0.25;
    expected[21] = -0.125;
    assert_eq!(pcm, expected);
    assert_eq!(report.counters.commands_consumed, 3);
    assert_eq!(report.counters.unknown_samples, 0);
    assert_eq!(report.counters.late_commands, 0);
}

#[test]
fn loader_refuses_missing_malformed_and_over_budget_assets_and_preserves_explicit_format_policy() {
    let text = "#BPM 60\n#WAV00 zero.wav\n#000D1:1E";
    let source = source(text);
    let selected = ids(&[0]);
    let missing = Assets::new(text, &[]);
    let decoder = Decoder::default();
    let error = bank(&source, &missing, &decoder).unwrap_err();
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::NotFound
    );
    assert!(missing.reads.borrow().is_empty() && decoder.calls.borrow().is_empty());
    for bytes in [
        b"RIFFbroken".to_vec(),
        wav(0, 1, &[0.5]),
        wav(10, 0, &[]),
        wav(10, 1, &[f32::NAN]),
        wav(10, 1, &[f32::INFINITY]),
        wav(10, 2, &[0.5, -0.5]),
    ] {
        let assets = Assets::new(text, &[("zero.wav", bytes)]);
        let decoder = Decoder::default();
        assert!(bank(&source, &assets, &decoder).is_err());
        assert_eq!(decoder.calls.borrow().len(), 1);
    }
    for kind in [io::ErrorKind::InvalidData, io::ErrorKind::UnexpectedEof] {
        let mut assets = Assets::new(text, &[("zero.wav", wav(10, 1, &[0.5, -0.25]))]);
        assets.read_error = Some(kind);
        let decoder = Decoder::default();
        let error = bank(&source, &assets, &decoder).unwrap_err();
        assert_eq!(error.downcast_ref::<io::Error>().unwrap().kind(), kind);
        assert_eq!(assets.reads.borrow()[0].1, 64 * 1024 * 1024);
        assert!(decoder.calls.borrow().is_empty());
    }
    let different_rate = Assets::new(text, &[("zero.wav", wav(20, 1, &[0.5, -0.25]))]);
    let loaded = bank(&source, &different_rate, &Decoder::default()).unwrap();
    assert_eq!(loaded.format(), format(1));
    assert_eq!(
        loaded.get(SampleId(0)).unwrap().format(),
        AudioFormat::new(20, 1).unwrap()
    );
    assert_eq!(loaded.get(SampleId(0)).unwrap().samples(), [0.5, -0.25]);
    for (policy, asset_limit, accepted) in [
        (ChannelPolicy::Exact, 16, false),
        (ChannelPolicy::MonoToStereo, 8, false),
        (ChannelPolicy::MonoToStereo, 16, true),
    ] {
        let assets = Assets::new(text, &[("zero.wav", wav(10, 1, &[0.5, -0.25]))]);
        let decoder = Decoder::default();
        let result = load_bank(
            &source,
            &selected,
            &assets,
            format(2),
            PcmLimits::new(asset_limit, 32, 1).unwrap(),
            policy,
            &decoder,
            AssetPathPolicy::Exact,
        );
        if accepted {
            assert_eq!(
                result.unwrap().get(SampleId(0)).unwrap().samples(),
                [0.5, 0.5, -0.25, -0.25]
            );
        } else {
            assert!(result.is_err());
        }
    }
    for ignored in [false, true] {
        let assets = Assets::new(text, &[("zero.wav", wav(10, 1, &[0.5, -0.25]))]);
        let decoder = Decoder {
            ignore_bound: ignored,
            ..Decoder::default()
        };
        let max_asset = if ignored { 8 } else { 4 };
        assert!(
            load_bank(
                &source,
                &selected,
                &assets,
                format(1),
                PcmLimits::new(max_asset, 8, 1).unwrap(),
                ChannelPolicy::Exact,
                &decoder,
                AssetPathPolicy::Exact
            )
            .is_err()
        );
        assert_eq!(decoder.calls.borrow().len(), 1);
    }
    let alias_text = "#BPM 60\n#WAV00 zero.wav\n#WAV01 ./zero.wav\n#00011:01\n#000D1:1E";
    let alias = self::source(alias_text);
    let union = ids(&[0, 1]);
    let assets = Assets::new(alias_text, &[("zero.wav", wav(10, 1, &[0.5, -0.25]))]);
    let decoder = Decoder::default();
    assert!(
        load_bank(
            &alias,
            &union,
            &assets,
            format(1),
            PcmLimits::new(8, 16, 1).unwrap(),
            ChannelPolicy::Exact,
            &decoder,
            AssetPathPolicy::Exact
        )
        .is_err()
    );
    no_io(&assets, &decoder);
    assert!(
        load_bank(
            &alias,
            &union,
            &assets,
            format(1),
            PcmLimits::new(8, 8, 2).unwrap(),
            ChannelPolicy::Exact,
            &decoder,
            AssetPathPolicy::Exact
        )
        .is_err()
    );
    assert_eq!(decoder.calls.borrow().len(), 1);
    assert_eq!(assets.reads.borrow().len(), 1);
    for invalid in [99, u64::MAX] {
        let assets = Assets::new(text, &[]);
        let decoder = Decoder::default();
        assert!(
            load_bank(
                &source,
                &ids(&[invalid]),
                &assets,
                format(1),
                bounds(),
                ChannelPolicy::Exact,
                &decoder,
                AssetPathPolicy::Exact
            )
            .is_err()
        );
        no_io(&assets, &decoder);
    }
    let escaped = self::source("#BPM 60\n#WAV00 ../zero.wav\n#000D1:1E");
    let assets = Assets::new(text, &[]);
    let decoder = Decoder::default();
    assert!(bank(&escaped, &assets, &decoder).is_err());
    assert!(assets.reads.borrow().is_empty() && decoder.calls.borrow().is_empty());
}

fn invalid_replay() -> ReplayFile {
    ReplayFile::new(
        ReplayHeader {
            version: 1,
            chart_identity: Vec::new(),
            rules_identity: Vec::new(),
            options: Vec::new(),
            seed: 0,
            normalized_clock: ClockDomainId(1),
        },
        Vec::new(),
    )
}
fn high_level(
    text: &str,
    assets: &Assets,
    decoder: &Decoder,
    limits: PcmLimits,
    replay: Option<&ReplayFile>,
) -> Result<PreparedBms, Box<dyn Error>> {
    let codec = ReplayCodecLimits::new(4096, 16, 2048, CodecLimits::new(4096, 1024)?)?;
    prepare_from_source(
        text.as_bytes(),
        assets,
        format(1),
        limits,
        ChannelPolicy::Exact,
        decoder,
        AssetPathPolicy::Exact,
        0,
        replay.map(|file| (file, codec)),
    )
}
#[test]
fn high_level_mine_guard_stays_before_replay_and_assets_while_ordinary_union_uses_shared_loader() {
    let bad_replay = invalid_replay();
    for suffix in [
        "#000D1:1E",
        "#000E9:ZZ",
        "#WAV00 absent.wav\n#000D1:1E",
        "#WAV00 absent.wav\n#000D1:ZZ",
    ] {
        let text = format!("#BPM 60\n#VOLWAV 50\n#WAV01 same.wav\n#00011:01\n{suffix}");
        for replay in [None, Some(&bad_replay)] {
            let assets = Assets::new(&text, &[]);
            let decoder = Decoder::default();
            let error = high_level(&text, &assets, &decoder, bounds(), replay).unwrap_err();
            assert_eq!(
                error.to_string(),
                "mine gameplay is not supported during preparation"
            );
            assert!(
                error.downcast_ref::<BmsError>().is_none()
                    && error.downcast_ref::<PlaybackError>().is_none()
            );
            no_io(&assets, &decoder);
        }
    }
    let invalid = "#000D1:01\n#VOLWAV bad";
    let assets = Assets::new(invalid, &[]);
    let decoder = Decoder::default();
    let error = high_level(invalid, &assets, &decoder, bounds(), Some(&bad_replay)).unwrap_err();
    assert_eq!(error.downcast_ref::<BmsError>().unwrap().line, 2);
    no_io(&assets, &decoder);
    let base = "#BASE 62\n#BPM 60\n#VOLWAV 50\n#WAV00 absent.wav\n#WAV01 same.wav\n#WAV02 same.wav\n#WAV0A ./same.wav\n#WAV0a same.wav\n#WAVzz same.wav\n#00011:01\n#00001:02\n#00031:0A0azz";
    for suffix in [
        "",
        "\n#000D1:0000",
        "\n#SETRANDOM 1\n#IF 2\n#000D1:1E\n#ENDIF",
    ] {
        let text = format!("{base}{suffix}");
        let assets = Assets::new(&text, &[("same.wav", wav(10, 1, &[0.5, -0.25]))]);
        let decoder = Decoder::default();
        let prepared = high_level(
            &text,
            &assets,
            &decoder,
            PcmLimits::new(8, 40, 5).unwrap(),
            None,
        )
        .unwrap();
        assert_eq!((prepared.bank.len(), prepared.bank.total_bytes()), (5, 40));
        assert!(prepared.bank.get(SampleId(0)).is_none());
        for id in [1, 2, 10, 36, 3843] {
            assert_eq!(
                prepared.bank.get(SampleId(id)).unwrap().samples(),
                [0.5, -0.25]
            );
        }
        assert_eq!((prepared.sounds.len(), prepared.bgm_commands.len()), (1, 1));
        assert_eq!(prepared.source.invisible.len(), 3);
        assert_eq!(prepared.sounds[0].gain, 0.5);
        assert_eq!(decoder.calls.borrow().len(), 1);
        assert_eq!(assets.reads.borrow().len(), 1);
        let rejected_assets = Assets::new(&text, &[]);
        let decoder = Decoder::default();
        let error = high_level(
            &text,
            &rejected_assets,
            &decoder,
            bounds(),
            Some(&bad_replay),
        )
        .unwrap_err();
        assert!(error.downcast_ref::<PlaybackError>().is_some());
        no_io(&rejected_assets, &decoder);
        assert!(
            high_level(
                &text,
                &rejected_assets,
                &decoder,
                PcmLimits::new(8, 40, 4).unwrap(),
                None
            )
            .is_err()
        );
        no_io(&rejected_assets, &decoder);
    }
}
