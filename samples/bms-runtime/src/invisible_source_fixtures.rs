//! Deferred source-byte admission and actual software composition fixtures.
//! Resource storage is in-memory; no native/browser/device execution is claimed.
use crate::{
    AssetDecoder, ChannelPolicy, PreparedBms,
    asset_paths::AssetPathPolicy,
    asset_source::{AssetSource, MemoryAssetLimits, MemoryFiles},
    native_audio::prepare_input_sounds,
    native_judge::{prepare_capture, prepare_capture_for_source},
    offline::{OfflineOptions, render_offline},
    prepare_from_source,
    replay_audio::plan_audio,
    replay_playback::PlaybackError,
    section_start::prepare_at,
    step_gameplay::{StepGameplay, StepGameplayConfig},
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
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    replay::codec::{ReplayCodecLimits, ReplayFile, decode_replay},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use std::{
    borrow::Cow,
    cell::RefCell,
    error::Error,
    io,
    path::{Path, PathBuf},
};

const LIVE: &str =
    "#BPM 60\n#VOLWAV 50\n#WAV01 one.pcm\n#WAV02 two.pcm\n#00031:01020000\n#00011:00000100";
const UNION: &str = "#BPM 60\n#WAV01 a.pcm\n#WAV02 b.pcm\n#WAV03 unused.pcm\n#WAV0A ./a.pcm\n#WAV0a c.pcm\n#WAVzz ./c.pcm\n#00011:01\n#00001:02\n#00031:0A0azz\n#00032:000100\n#BASE 62";
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(ns),
    }
}
fn format(channels: u16) -> AudioFormat {
    AudioFormat::new(10, channels).unwrap()
}
fn bounds() -> PcmLimits {
    PcmLimits::new(64, 128, 8).unwrap()
}
fn replay_limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 64, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
struct Assets {
    files: MemoryFiles,
    resolved: RefCell<Vec<(String, AssetPathPolicy)>>,
    reads: RefCell<Vec<(PathBuf, usize)>>,
}
impl Assets {
    fn new(text: &str, files: &[(&str, &[u8])]) -> Self {
        let mut memory = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
        memory
            .insert("pack/chart.bms", text.as_bytes().to_vec())
            .unwrap();
        for &(name, bytes) in files {
            memory
                .insert(&format!("pack/{name}"), bytes.to_vec())
                .unwrap();
        }
        Self {
            files: memory,
            resolved: RefCell::new(vec![]),
            reads: RefCell::new(vec![]),
        }
    }
    fn live(text: &str) -> Self {
        Self::new(text, &[("one.pcm", &[1]), ("two.pcm", &[2])])
    }
}
impl AssetSource for Assets {
    fn resolve(&self, name: &str, policy: AssetPathPolicy) -> io::Result<PathBuf> {
        self.resolved.borrow_mut().push((name.into(), policy));
        self.files.scope("pack/chart.bms")?.resolve(name, policy)
    }
    fn read<'a>(&'a self, key: &Path, max_bytes: usize) -> io::Result<Cow<'a, [u8]>> {
        self.reads.borrow_mut().push((key.into(), max_bytes));
        // Keep the actual scoped read/containment checks; own only this wrapper's return.
        Ok(Cow::Owned(
            self.files
                .scope("pack/chart.bms")?
                .read(key, max_bytes)?
                .into_owned(),
        ))
    }
}
#[derive(Default)]
struct Decoder {
    calls: RefCell<Vec<PathBuf>>,
}
impl AssetDecoder for Decoder {
    fn decode(
        &self,
        path: &Path,
        encoded: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>> {
        self.calls.borrow_mut().push(path.into());
        let (channels, pcm) = match encoded {
            [1] => (1, vec![0.5, -0.25]),
            [2] => (1, vec![0.25, 0.75]),
            [3] => (2, vec![0.5, -0.5, 0.25, -0.25]),
            [255] => return Err("original controlled decode failure".into()),
            _ => panic!("unexpected fixture asset bytes"),
        };
        Ok(PcmSample::new(format(channels), pcm, limits)?)
    }
}
fn prepare_options(
    assets: &Assets,
    decoder: &Decoder,
    pcm: PcmLimits,
    output: AudioFormat,
    channels: ChannelPolicy,
    paths: AssetPathPolicy,
    replay: Option<&ReplayFile>,
) -> Result<PreparedBms, Box<dyn Error>> {
    let bytes = assets.files.read_file("pack/chart.bms", 8 * 1024 * 1024)?;
    prepare_from_source(
        bytes,
        assets,
        output,
        pcm,
        channels,
        decoder,
        paths,
        0,
        replay.map(|file| (file, replay_limits())),
    )
}
fn prepare(assets: &Assets, decoder: &Decoder) -> PreparedBms {
    prepare_options(
        assets,
        decoder,
        bounds(),
        format(1),
        ChannelPolicy::Exact,
        AssetPathPolicy::Exact,
        None,
    )
    .unwrap()
}
fn no_assets(assets: &Assets, decoder: &Decoder) {
    assert!(assets.resolved.borrow().is_empty());
    assert!(assets.reads.borrow().is_empty());
    assert!(decoder.calls.borrow().is_empty());
}
fn union_assets() -> Assets {
    Assets::new(UNION, &[("a.pcm", &[1]), ("b.pcm", &[2]), ("c.pcm", &[1])])
}

#[test]
fn source_bytes_load_the_unique_original_union_with_base62_aliases_and_pre_io_sample_count() {
    let assets = union_assets();
    let decoder = Decoder::default();
    let prepared = prepare_options(
        &assets,
        &decoder,
        PcmLimits::new(8, 40, 5).unwrap(),
        format(1),
        ChannelPolicy::Exact,
        AssetPathPolicy::Exact,
        None,
    )
    .unwrap();
    assert_eq!((prepared.bank.len(), prepared.bank.total_bytes()), (5, 40));
    for id in [1, 2, 10, 36, 3843] {
        assert!(prepared.bank.get(SampleId(id)).is_some());
    }
    assert!(prepared.bank.get(SampleId(3)).is_none());
    assert_eq!(prepared.source.invisible.len(), 4);
    assert_eq!(
        (
            prepared.source.notes.len(),
            prepared.sounds.len(),
            prepared.bgm_commands.len()
        ),
        (1, 1, 1)
    );
    assert_eq!(
        assets
            .resolved
            .borrow()
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["a.pcm", "b.pcm", "./a.pcm", "c.pcm", "./c.pcm"]
    );
    assert_eq!(
        *decoder.calls.borrow(),
        [
            PathBuf::from("pack/a.pcm"),
            PathBuf::from("pack/b.pcm"),
            PathBuf::from("pack/c.pcm")
        ]
    );
    assert_eq!(assets.reads.borrow().len(), 3);
    assert!(
        assets
            .reads
            .borrow()
            .iter()
            .all(|(_, bound)| *bound == 64 * 1024 * 1024)
    );
    assert_eq!(
        prepared.bank.get(SampleId(1)).unwrap().samples(),
        prepared.bank.get(SampleId(10)).unwrap().samples()
    );
    assert_ne!(
        prepared.bank.get(SampleId(1)).unwrap().samples().as_ptr(),
        prepared.bank.get(SampleId(10)).unwrap().samples().as_ptr()
    );
    let original = prepared.source.invisible.clone();
    let pointer = prepared.bank.get(SampleId(10)).unwrap().samples().as_ptr();
    let (selected, report) = prepare_at(
        prepared,
        ts(3_000_000_000),
        PcmLimits::new(8, 40, 5).unwrap(),
    )
    .unwrap();
    assert_eq!(selected.source.invisible, original);
    assert!(selected.source.notes.is_empty() && selected.bgm_commands.is_empty());
    assert_eq!((report.excluded_objects, report.retired_bgm), (1, 1));
    assert_eq!(selected.bank.len(), 5);
    assert_eq!(
        selected.bank.get(SampleId(10)).unwrap().samples().as_ptr(),
        pointer
    );
    let assets = union_assets();
    let decoder = Decoder::default();
    assert!(
        prepare_options(
            &assets,
            &decoder,
            PcmLimits::new(8, 40, 4).unwrap(),
            format(1),
            ChannelPolicy::Exact,
            AssetPathPolicy::Exact,
            None
        )
        .is_err()
    );
    no_assets(&assets, &decoder);
    // Alias reuse saves decode work, never per-SampleId owned PCM budget.
    let assets = union_assets();
    let decoder = Decoder::default();
    assert!(
        prepare_options(
            &assets,
            &decoder,
            PcmLimits::new(8, 32, 5).unwrap(),
            format(1),
            ChannelPolicy::Exact,
            AssetPathPolicy::Exact,
            None
        )
        .is_err()
    );
    assert_eq!(decoder.calls.borrow().len(), 3);
}

fn pristine(prepared: &PreparedBms) -> JudgeEngine {
    JudgeEngine::new(
        prepared.compiled.chart.clone(),
        prepared.source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap()
}
#[test]
fn source_failures_preserve_asset_policies_pcm_bounds_and_replay_validation_before_resource_io() {
    let only = "#BPM 60\n#WAV01 one.pcm\n#00031:01";
    let missing = Assets::new(only, &[]);
    let decoder = Decoder::default();
    let error = prepare_options(
        &missing,
        &decoder,
        bounds(),
        format(1),
        ChannelPolicy::Exact,
        AssetPathPolicy::Exact,
        None,
    )
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(missing.resolved.borrow().len(), 1);
    assert!(missing.reads.borrow().is_empty() && decoder.calls.borrow().is_empty());
    let bad = Assets::new(only, &[("one.pcm", &[255])]);
    let decoder = Decoder::default();
    assert_eq!(
        prepare_options(
            &bad,
            &decoder,
            bounds(),
            format(1),
            ChannelPolicy::Exact,
            AssetPathPolicy::Exact,
            None
        )
        .unwrap_err()
        .to_string(),
        "original controlled decode failure"
    );
    let assets = Assets::live(only);
    let decoder = Decoder::default();
    assert!(
        prepare_options(
            &assets,
            &decoder,
            PcmLimits::new(4, 8, 1).unwrap(),
            format(1),
            ChannelPolicy::Exact,
            AssetPathPolicy::Exact,
            None
        )
        .is_err()
    );
    assert_eq!(decoder.calls.borrow().len(), 1);
    for (policy, asset_limit, succeeds) in [
        (ChannelPolicy::Exact, 16, false),
        (ChannelPolicy::MonoToStereo, 8, false),
        (ChannelPolicy::MonoToStereo, 16, true),
    ] {
        let assets = Assets::live(only);
        let decoder = Decoder::default();
        let result = prepare_options(
            &assets,
            &decoder,
            PcmLimits::new(asset_limit, 32, 1).unwrap(),
            format(2),
            policy,
            AssetPathPolicy::Exact,
            None,
        );
        if succeeds {
            assert_eq!(
                result.unwrap().bank.get(SampleId(1)).unwrap().samples(),
                [0.5, 0.5, -0.25, -0.25]
            );
        } else {
            assert!(result.is_err());
        }
        assert_eq!(decoder.calls.borrow().len(), 1);
    }
    let variant = only.replace("one.pcm", "sound.ogg");
    for policy in [AssetPathPolicy::Exact, AssetPathPolicy::AudioVariants] {
        let assets = Assets::new(&variant, &[("sound.wav", &[1])]);
        let decoder = Decoder::default();
        let result = prepare_options(
            &assets,
            &decoder,
            bounds(),
            format(1),
            ChannelPolicy::Exact,
            policy,
            None,
        );
        if policy == AssetPathPolicy::Exact {
            assert!(result.is_err());
            assert!(decoder.calls.borrow().is_empty());
        } else {
            assert_eq!(result.unwrap().bank.len(), 1);
            assert_eq!(*decoder.calls.borrow(), [PathBuf::from("pack/sound.wav")]);
        }
    }
    let escape = Assets::new(&only.replace("one.pcm", "../one.pcm"), &[]);
    let decoder = Decoder::default();
    assert!(
        prepare_options(
            &escape,
            &decoder,
            bounds(),
            format(1),
            ChannelPolicy::Exact,
            AssetPathPolicy::AudioVariants,
            None
        )
        .is_err()
    );
    assert!(escape.reads.borrow().is_empty() && decoder.calls.borrow().is_empty());
    let invalid_gain = Assets::live(&format!("{only}\n#VOLWAV bad"));
    let decoder = Decoder::default();
    assert!(
        prepare_options(
            &invalid_gain,
            &decoder,
            bounds(),
            format(1),
            ChannelPolicy::Exact,
            AssetPathPolicy::Exact,
            None
        )
        .is_err()
    );
    no_assets(&invalid_gain, &decoder);

    let good = prepare(&Assets::live(LIVE), &Decoder::default());
    let judge = pristine(&good);
    let aware = prepare_capture_for_source(
        &good.source,
        &judge,
        ClockDomainId(1),
        ts(0),
        0,
        Some(replay_limits()),
    )
    .unwrap()
    .unwrap()
    .into_file();
    let legacy = prepare_capture(&judge, ClockDomainId(1), ts(0), 0, Some(replay_limits()))
        .unwrap()
        .unwrap()
        .into_file();
    let mut forged = aware.clone();
    forged.header.chart_identity[0] ^= 1;
    for (text, file) in [
        (LIVE.to_owned(), legacy),
        (LIVE.to_owned(), forged),
        (
            LIVE.replace("#00031:01020000", "#00031:02020000"),
            aware.clone(),
        ),
    ] {
        let assets = Assets::live(&text);
        let decoder = Decoder::default();
        let error = prepare_options(
            &assets,
            &decoder,
            bounds(),
            format(1),
            ChannelPolicy::Exact,
            AssetPathPolicy::Exact,
            Some(&file),
        )
        .unwrap_err();
        assert!(error.downcast_ref::<PlaybackError>().is_some());
        no_assets(&assets, &decoder);
    }
    let assets = Assets::live(LIVE);
    let decoder = Decoder::default();
    assert_eq!(
        prepare_options(
            &assets,
            &decoder,
            bounds(),
            format(1),
            ChannelPolicy::Exact,
            AssetPathPolicy::Exact,
            Some(&aware)
        )
        .unwrap()
        .bank
        .len(),
        2
    );
    assert_eq!(decoder.calls.borrow().len(), 2);
}

fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(1, 10_000_000_000),
        output_origin: point(2, 1_000_000_000),
        preroll: Duration::from_nanos(100_000_000),
        early_ns: 0,
        late_ns: 0,
        offset_ns: 100_000_000,
        command_capacity: 32,
        bgm_pending: 4,
        bgm_lookahead: Duration::from_nanos(1_000_000_000),
        telemetry_capacity: 0,
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
fn play(sample: u64, voice: u64, at: i64) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(sample),
        voice: VoiceId(voice),
        at: ts(at),
        gain: 0.5,
    }
}
fn mix(bank: SampleBank, commands: &[AudioCommand]) -> Vec<f32> {
    let (mut producer, consumer) = command_queue(32).unwrap();
    for command in commands {
        producer.try_push(*command).unwrap();
    }
    let mut mixer = Mixer::new(
        MixerConfig::new(
            bank.format(),
            ClockDomainId(2),
            ts(1_000_000_000),
            AudioLimits::new(32, 8, 32, 32, 32).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let mut output = vec![9.0; 30];
    mixer.render(&mut output).unwrap();
    output
}
#[test]
fn admitted_source_flows_through_practice_step_native_plan_capture_replay_and_software_pcm() {
    let original = prepare(&Assets::live(LIVE), &Decoder::default());
    let (selected, _) = prepare_at(original, ts(500_000_000), bounds()).unwrap();
    let native = prepare_input_sounds(&selected).unwrap().unwrap();
    assert_eq!(
        native.command_for(GameControlId(0x11), ts(500_000_000), ts(1_100_000_000)),
        Some(play(1, 2, 1_100_000_000))
    );
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(91),
        game_control: GameControlId(0x11),
    }])
    .unwrap();
    let (mut owner, bank) =
        StepGameplay::new_at(selected, config(), bindings, ts(500_000_000)).unwrap();
    owner.configure_capture(replay_limits(), 0).unwrap();
    owner.activate(config().host_origin).unwrap();
    let mut actual = Vec::new();
    let mut judged = Vec::new();
    for (sequence, (song, state)) in [
        (500_000_000, ButtonState::Down),
        (600_000_000, ButtonState::Up),
        (1_000_000_000, ButtonState::Down),
        (1_100_000_000, ButtonState::Down),
        (1_200_000_000, ButtonState::Up),
        (1_900_000_000, ButtonState::Down),
        (2_000_000_000, ButtonState::Up),
    ]
    .into_iter()
    .enumerate()
    {
        let event = PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(
                DeviceId(u64::MAX),
                point(1, 10_000_000_000 + song - 400_000_000),
                sequence as u64,
            ),
            control: PhysicalControlId::keyboard(91),
            state,
        });
        let report = owner
            .process_input(
                event.clone(),
                &NoMapping,
                point(2, 1_000_000_000 + song - 400_000_000),
            )
            .unwrap();
        assert_eq!(report.song_time, ts(song));
        assert_eq!(report.bound_inputs[0].physical, event);
        assert!(report.audio_failures.is_empty() && report.judge_error.is_none());
        actual.extend(report.audio_commands);
        judged.extend(report.judge_events);
    }
    let expected = vec![
        play(1, 2, 1_100_000_000),
        play(2, 2, 1_600_000_000),
        play(1, 1, 2_500_000_000),
    ];
    assert_eq!(actual, expected);
    assert_eq!(judged.len(), 1);
    let batch = owner.take_commands(32).unwrap().unwrap();
    assert_eq!(batch.commands, expected);
    owner.acknowledge(batch.sequence, 3, true).unwrap();
    let hash = owner.judge().stable_hash().unwrap();
    owner.fail();
    let captured = owner.take_replay().unwrap().unwrap();
    assert!(owner.take_replay().unwrap().is_none());
    let file = decode_replay(&captured, replay_limits()).unwrap();
    assert!(
        file.header
            .chart_identity
            .starts_with(b"bms-judge-setup/v2:")
    );
    let assets = Assets::live(LIVE);
    let decoder = Decoder::default();
    let original = prepare_options(
        &assets,
        &decoder,
        bounds(),
        format(1),
        ChannelPolicy::Exact,
        AssetPathPolicy::Exact,
        Some(&file),
    )
    .unwrap();
    assert_eq!(original.bank.len(), 2);
    let (selected, _) = prepare_at(original, ts(500_000_000), bounds()).unwrap();
    let replay = plan_audio(
        &selected,
        file,
        replay_limits(),
        config().output_origin,
        config().preroll,
    )
    .unwrap();
    assert_eq!(replay.commands, expected);
    assert_eq!(replay.judge_events, judged);
    assert_eq!(replay.final_judge_hash, hash);
    assert_eq!(replay.recorded_until, Some(ts(2_000_000_000)));
    let mut pcm = vec![0.0; 30];
    pcm[1..3].copy_from_slice(&[0.25, -0.125]);
    pcm[6..8].copy_from_slice(&[0.125, 0.375]);
    pcm[15..17].copy_from_slice(&[0.25, -0.125]);
    assert_eq!(mix(bank, &actual), pcm);
    assert_eq!(mix(selected.bank, &replay.commands), pcm);
}

fn offline_options(frames: u64, block_frames: usize) -> OfflineOptions {
    OfflineOptions {
        frames,
        block_frames,
        command_capacity: 32,
        max_voices: 8,
    }
}
fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
        .collect()
}
#[test]
fn offline_uses_real_admitted_sources_without_generating_invisible_events_or_bypassing_zero_frame_validation()
 {
    let invisible = "#BPM 60\n#WAV01 one.pcm\n#WAV02 two.pcm\n#00031:0102\n#00032:02";
    for block in [1, 7, 32] {
        let mut bytes = vec![];
        let report = render_offline(
            prepare(&Assets::live(invisible), &Decoder::default()),
            offline_options(30, block),
            &mut bytes,
        )
        .unwrap();
        assert_eq!(
            (report.frames, report.hits, report.judge_results),
            (30, 0, 0)
        );
        assert_eq!(bytes, vec![0; 120]);
        assert_eq!(report.last_render.unwrap().counters.commands_consumed, 0);
        let mut mixed_bytes = vec![];
        let mixed = render_offline(
            prepare(&Assets::live(LIVE), &Decoder::default()),
            offline_options(30, block),
            &mut mixed_bytes,
        )
        .unwrap();
        assert_eq!((mixed.hits, mixed.judge_results), (1, 1));
        assert_eq!(mixed.last_render.unwrap().counters.commands_consumed, 1);
        let ordinary = LIVE.replace("#00031:01020000\n", "");
        let mut legacy = vec![];
        let old = render_offline(
            prepare(&Assets::live(&ordinary), &Decoder::default()),
            offline_options(30, block),
            &mut legacy,
        )
        .unwrap();
        assert_eq!((old.hits, old.judge_results), (1, 1));
        assert_eq!(mixed_bytes, legacy);
        let mut expected = vec![0.0; 30];
        expected[20..22].copy_from_slice(&[0.25, -0.125]);
        assert_eq!(floats(&mixed_bytes), expected);
    }
    let mut untouched = vec![7, 8];
    let zero = render_offline(
        prepare(&Assets::live(invisible), &Decoder::default()),
        offline_options(0, 8),
        &mut untouched,
    )
    .unwrap();
    assert_eq!((zero.frames, zero.hits, zero.judge_results), (0, 0, 0));
    assert!(zero.last_render.is_none());
    assert_eq!(untouched, [7, 8]);
    for invalid_grid in [false, true] {
        let mut bad = prepare(&Assets::live(invisible), &Decoder::default());
        if invalid_grid {
            bad.source.invisible_ticks_per_beat = 0;
        } else {
            bad.bank = SampleBank::new(format(1), bounds()).unwrap();
        }
        assert!(render_offline(bad, offline_options(0, 8), &mut untouched).is_err());
        assert_eq!(untouched, [7, 8]);
    }
}
