//! Actual chart/PCM/image preparation through selected in-memory assets.
use crate::{
    AssetDecoder, ChannelPolicy, PreparedBms, WavDecoder,
    asset_paths::AssetPathPolicy,
    asset_source::{AssetSource, MemoryFiles},
    image_assets::{ImageAssetLimits, ImageAssets, ImageUnavailable},
    prepare_from_source,
    replay_capture::LiveReplayCapture,
};
use beatkernel::{
    audio::{AudioCommand, AudioError, AudioFormat, PcmLimits, PcmSample, SampleId, VoiceId},
    input::CodecLimits,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow},
    replay::codec::{ReplayCodecLimits, ReplayFile},
    time::{ClockDomainId, Duration, Timestamp},
};
use beatkernel_bms::{ImageId, parse, parse_seeded};
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    error::Error,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

fn wav(rate: u32, samples: &[i16]) -> Vec<u8> {
    let data_bytes = u32::try_from(samples.len() * 2).unwrap();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn bmp(rgb: [u8; 3]) -> Vec<u8> {
    // Original 1x1, 24-bit BMP, including one padded scanline.
    let mut bytes = vec![0; 58];
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&58u32.to_le_bytes());
    bytes[10..14].copy_from_slice(&54u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&1i32.to_le_bytes());
    bytes[22..26].copy_from_slice(&1i32.to_le_bytes());
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&4u32.to_le_bytes());
    bytes[54..57].copy_from_slice(&[rgb[2], rgb[1], rgb[0]]);
    bytes
}

fn pcm_limits() -> PcmLimits {
    PcmLimits::new(1024, 4096, 8).unwrap()
}

fn selected(chart: &[u8]) -> MemoryFiles {
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files.insert("pack/chart.bms", chart.to_vec()).unwrap();
    files
}

fn prepare(
    chart: &[u8],
    source: &dyn AssetSource,
    channels: ChannelPolicy,
    output_channels: u16,
    decoder: &dyn AssetDecoder,
) -> Result<PreparedBms, Box<dyn Error>> {
    prepare_from_source(
        chart,
        source,
        AudioFormat::new(48_000, output_channels).unwrap(),
        pcm_limits(),
        channels,
        decoder,
        AssetPathPolicy::AudioVariants,
        0,
        None,
    )
}

#[derive(Default)]
struct RecordingDecoder(RefCell<Vec<PathBuf>>);
impl AssetDecoder for RecordingDecoder {
    fn decode(
        &self,
        path: &Path,
        encoded: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>> {
        self.0.borrow_mut().push(path.to_owned());
        WavDecoder.decode(path, encoded, limits)
    }
}

struct CountingSource<'a> {
    inner: &'a dyn AssetSource,
    resolves: Cell<usize>,
    reads: Cell<usize>,
}
impl<'a> CountingSource<'a> {
    fn new(inner: &'a dyn AssetSource) -> Self {
        Self {
            inner,
            resolves: Cell::new(0),
            reads: Cell::new(0),
        }
    }
    fn counts(&self) -> (usize, usize) {
        (self.resolves.get(), self.reads.get())
    }
}
impl AssetSource for CountingSource<'_> {
    fn resolve(&self, name: &str, policy: AssetPathPolicy) -> io::Result<PathBuf> {
        self.resolves.set(self.resolves.get() + 1);
        self.inner.resolve(name, policy)
    }
    fn read<'a>(&'a self, key: &Path, max_bytes: usize) -> io::Result<Cow<'a, [u8]>> {
        self.reads.set(self.reads.get() + 1);
        self.inner.read(key, max_bytes)
    }
}

#[test]
fn resolved_audio_aliases_decode_once_but_keep_distinct_owned_expanded_pcm_and_sample_ids() {
    let chart = "#BPM 120\n#WAV01 音.WAV\n#WAV02 音.WAV\n#WAV03 ./音.WAV\n#WAV04 音.ogg\n#00011:0102\n#00012:03\n#00001:04\n";
    let mut files = selected(chart.as_bytes());
    files
        .insert("pack/音.WAV", wav(24_000, &[0, 16_384, -16_384]))
        .unwrap();
    let memory = files.scope("pack/chart.bms").unwrap();
    let source = CountingSource::new(&memory);
    let decoder = RecordingDecoder::default();
    let prepared = prepare(
        chart.as_bytes(),
        &source,
        ChannelPolicy::MonoToStereo,
        2,
        &decoder,
    )
    .unwrap();
    assert_eq!(
        source.counts(),
        (4, 1),
        "each original name still passes path admission"
    );
    assert_eq!(
        decoder.0.borrow().as_slice(),
        &[PathBuf::from("pack/音.WAV")]
    );
    assert_eq!((prepared.bank.len(), prepared.bank.total_bytes()), (4, 96));
    let mut pointers = Vec::new();
    for id in 1..=4 {
        let sample = prepared.bank.get(SampleId(id)).unwrap();
        assert_eq!(sample.format(), AudioFormat::new(24_000, 2).unwrap());
        assert_eq!(sample.frames(), 3);
        assert_eq!(sample.samples(), &[0.0, 0.0, 0.5, 0.5, -0.5, -0.5]);
        assert!(!pointers.contains(&sample.samples().as_ptr()));
        pointers.push(sample.samples().as_ptr());
    }
    let sounded: std::collections::BTreeSet<_> =
        prepared.sounds.iter().map(|sound| sound.sample).collect();
    assert_eq!(
        sounded,
        [SampleId(1), SampleId(2), SampleId(3)]
            .into_iter()
            .collect()
    );
    assert!(matches!(
        prepared.bgm_commands.as_slice(),
        [AudioCommand::Play {
            sample: SampleId(4),
            ..
        }]
    ));
    let another = prepare(
        chart.as_bytes(),
        &source,
        ChannelPolicy::MonoToStereo,
        2,
        &decoder,
    )
    .unwrap();
    assert_eq!(
        source.counts(),
        (8, 2),
        "decoded reuse ends with its preparation call"
    );
    assert_eq!(decoder.0.borrow().len(), 2);
    assert_ne!(
        another.bank.get(SampleId(1)).unwrap().samples().as_ptr(),
        pointers[0]
    );
}

#[test]
fn audio_reuse_obeys_exact_resolved_keys_and_original_reference_failures() {
    let chart = b"#BPM 120\n#WAV01 first.wav\n#WAV02 second.wav\n#00011:0102\n";
    let mut files = selected(chart);
    let encoded = wav(44_100, &[8192]);
    files.insert("pack/first.wav", encoded.clone()).unwrap();
    files.insert("pack/second.wav", encoded).unwrap();
    let memory = files.scope("pack/chart.bms").unwrap();
    let source = CountingSource::new(&memory);
    let decoder = RecordingDecoder::default();
    let prepared = prepare(chart, &source, ChannelPolicy::Exact, 1, &decoder).unwrap();
    assert_eq!(
        source.counts(),
        (2, 2),
        "identical content at distinct keys is not a cache hit"
    );
    assert_eq!(
        decoder.0.borrow().as_slice(),
        &[
            PathBuf::from("pack/first.wav"),
            PathBuf::from("pack/second.wav")
        ]
    );
    assert_eq!(prepared.bank.total_bytes(), 8);

    for (second, policy, expected_counts, expected_decodes) in [
        ("first.ogg", AssetPathPolicy::Exact, (2, 1), 1),
        ("../first.wav", AssetPathPolicy::AudioVariants, (2, 1), 1),
        ("broken.wav", AssetPathPolicy::AudioVariants, (2, 2), 2),
    ] {
        let chart = format!("#BPM 120\n#WAV01 first.wav\n#WAV02 {second}\n#00011:0102\n");
        let mut files = selected(chart.as_bytes());
        files
            .insert("pack/first.wav", wav(44_100, &[8192]))
            .unwrap();
        files
            .insert("pack/broken.wav", b"bad literal".to_vec())
            .unwrap();
        files
            .insert("pack/broken.FLAC", wav(44_100, &[8192]))
            .unwrap();
        let memory = files.scope("pack/chart.bms").unwrap();
        let source = CountingSource::new(&memory);
        let decoder = RecordingDecoder::default();
        assert!(
            prepare_from_source(
                chart.as_bytes(),
                &source,
                AudioFormat::new(48_000, 1).unwrap(),
                pcm_limits(),
                ChannelPolicy::Exact,
                &decoder,
                policy,
                0,
                None
            )
            .is_err()
        );
        assert_eq!(source.counts(), expected_counts);
        assert_eq!(decoder.0.borrow().len(), expected_decodes);
        assert!(
            !decoder
                .0
                .borrow()
                .iter()
                .any(|path| path == Path::new("pack/broken.FLAC")),
            "a decode failure cannot reinterpret the selected literal as another resource"
        );
    }
}

#[test]
fn aliased_assets_charge_full_pcm_bytes_and_count_after_expansion_without_redecoding() {
    let chart = b"#BPM 120\n#WAV01 tap.wav\n#WAV02 ./tap.wav\n#00011:0102\n";
    let mut files = selected(chart);
    files
        .insert("pack/tap.wav", wav(24_000, &[0, 16_384, -16_384]))
        .unwrap();
    let memory = files.scope("pack/chart.bms").unwrap();
    for (asset, total, count, expected_counts, accepted) in [
        (24, 48, 1, (0, 0), false),
        (23, 48, 2, (1, 1), false),
        (24, 47, 2, (2, 1), false),
        (24, 48, 2, (2, 1), true),
    ] {
        let source = CountingSource::new(&memory);
        let decoder = RecordingDecoder::default();
        let result = prepare_from_source(
            chart,
            &source,
            AudioFormat::new(48_000, 2).unwrap(),
            PcmLimits::new(asset, total, count).unwrap(),
            ChannelPolicy::MonoToStereo,
            &decoder,
            AssetPathPolicy::Exact,
            0,
            None,
        );
        assert_eq!(source.counts(), expected_counts);
        assert_eq!(decoder.0.borrow().len(), expected_counts.1);
        if accepted {
            let prepared = result.unwrap();
            assert_eq!((prepared.bank.len(), prepared.bank.total_bytes()), (2, 48));
            assert_eq!(
                prepared.bank.get(SampleId(2)).unwrap().samples(),
                &[0.0, 0.0, 0.5, 0.5, -0.5, -0.5]
            );
        } else {
            let error = result
                .err()
                .expect("alias admission must not bypass the full bank budget");
            if total == 47 {
                assert_eq!(
                    error.downcast_ref::<AudioError>(),
                    Some(&AudioError::PcmCapacity)
                );
            }
            if asset == 23 {
                assert!(error.to_string().contains("stereo expansion"));
            }
        }
    }
}

#[test]
fn selected_wav_prepares_actual_notes_bgm_and_original_voice_and_sample_identities() {
    let chart = "#TITLE 選択曲\n#BPM 120\n#VOLWAV 50\n#WAV01 鍵.wav\n#WAV02 bgm.wav\n#WAV03 missing.wav\n#00111:01\n#00001:0202\n";
    let mut files = selected(chart.as_bytes());
    files
        .insert("pack/鍵.WAV", wav(24_000, &[0, 16_384, -16_384]))
        .unwrap();
    files.insert("pack/bgm.wav", wav(44_100, &[8_192])).unwrap();
    let memory = files.scope("pack/chart.bms").unwrap();
    let source = CountingSource::new(&memory);
    let decoder = RecordingDecoder::default();
    let prepared = prepare(chart.as_bytes(), &source, ChannelPolicy::Exact, 1, &decoder).unwrap();
    assert_eq!(source.counts(), (2, 2));
    assert_eq!(prepared.source.samples.get(&1).unwrap(), "鍵.wav");
    assert_eq!(prepared.bank.len(), 2);
    assert!(prepared.bank.get(SampleId(3)).is_none());
    let tap = prepared.bank.get(SampleId(1)).unwrap();
    assert_eq!(tap.samples(), &[0.0, 0.5, -0.5]);
    assert_eq!(tap.frames(), 3);
    assert_eq!(tap.format(), AudioFormat::new(24_000, 1).unwrap());
    assert_eq!(
        prepared
            .bank
            .get(SampleId(2))
            .unwrap()
            .format()
            .sample_rate(),
        44_100
    );
    assert_eq!(prepared.bank.format(), AudioFormat::new(48_000, 1).unwrap());
    assert_eq!(
        decoder.0.borrow().as_slice(),
        &[PathBuf::from("pack/鍵.WAV"), PathBuf::from("pack/bgm.wav")]
    );
    assert_eq!(prepared.sounds.len(), 1);
    let note = &prepared.source.notes[0];
    let sound = &prepared.sounds[0];
    assert_eq!(
        (sound.object, sound.sample, sound.voice),
        (note.object, SampleId(1), VoiceId(note.object.0))
    );
    assert_eq!(sound.stage, JudgeStage::Instant);
    assert_eq!(sound.gain, 0.5);
    assert_eq!(prepared.bgm_commands.len(), 2);
    for (index, command) in prepared.bgm_commands.iter().enumerate() {
        let AudioCommand::Play {
            voice,
            sample,
            at,
            gain,
        } = command
        else {
            panic!("BGM must prepare a play command");
        };
        assert_eq!(*sample, SampleId(2));
        assert_eq!(*at, Timestamp::from_nanos(index as i64 * 1_000_000_000));
        assert_eq!(*gain, 0.5);
        assert_eq!(*voice, VoiceId(sound.voice.0 + 1 + index as u64));
        assert_eq!(*at, prepared.compiled.bgm[index].at);
    }
}

#[test]
fn channel_expansion_is_explicit_and_pcm_limits_apply_to_expanded_storage() {
    let chart = b"#BPM 120\n#WAV01 tap.wav\n#00011:01\n";
    let mut files = selected(chart);
    files
        .insert("pack/tap.wav", wav(24_000, &[0, 16_384, -16_384]))
        .unwrap();
    let source = files.scope("pack/chart.bms").unwrap();
    assert!(prepare(chart, &source, ChannelPolicy::Exact, 2, &WavDecoder).is_err());
    let prepared = prepare(chart, &source, ChannelPolicy::MonoToStereo, 2, &WavDecoder).unwrap();
    let tap = prepared.bank.get(SampleId(1)).unwrap();
    assert_eq!(tap.samples(), &[0.0, 0.0, 0.5, 0.5, -0.5, -0.5]);
    assert_eq!((tap.frames(), prepared.bank.total_bytes()), (3, 24));
    assert_eq!(tap.format(), AudioFormat::new(24_000, 2).unwrap());
    assert!(prepare(chart, &source, ChannelPolicy::MonoToStereo, 3, &WavDecoder).is_err());
    for cap in [12, 23, 24] {
        let result = prepare_from_source(
            chart,
            &source,
            AudioFormat::new(48_000, 2).unwrap(),
            PcmLimits::new(cap, cap, 1).unwrap(),
            ChannelPolicy::MonoToStereo,
            &WavDecoder,
            AssetPathPolicy::Exact,
            0,
            None,
        );
        assert_eq!(result.is_ok(), cap == 24);
    }
}

#[test]
fn literal_decode_errors_do_not_retry_variants_and_seed_selects_only_referenced_assets() {
    let chart = b"#BPM 120\n#WAV01 tap.wav\n#00011:01\n";
    let mut files = selected(chart);
    files
        .insert("pack/tap.wav", b"malformed literal".to_vec())
        .unwrap();
    files.insert("pack/tap.FLAC", wav(48_000, &[1])).unwrap();
    let source = files.scope("pack/chart.bms").unwrap();
    let decoder = RecordingDecoder::default();
    assert!(prepare(chart, &source, ChannelPolicy::Exact, 1, &decoder).is_err());
    assert_eq!(
        decoder.0.borrow().as_slice(),
        &[PathBuf::from("pack/tap.wav")]
    );

    let branch = "#BPM 120\n#RANDOM 2\n#IF 1\n#WAV01 first.wav\n#00011:01\n#ELSE\n#WAV02 second.wav\n#00112:02\n#ENDIF\n#ENDRANDOM\n";
    for seed in [0, 1, 37, u64::MAX] {
        let expected = parse_seeded(branch, Default::default(), seed).unwrap();
        assert_eq!(expected.notes.len(), 1);
        let selected_sample = expected.notes[0].sample;
        let name = expected
            .samples
            .get(&u16::try_from(selected_sample.0).unwrap())
            .unwrap();
        let mut files = selected(branch.as_bytes());
        files
            .insert(&format!("pack/{name}"), wav(48_000, &[8_192]))
            .unwrap();
        let source = files.scope("pack/chart.bms").unwrap();
        let prepared = prepare_from_source(
            branch.as_bytes(),
            &source,
            AudioFormat::new(48_000, 1).unwrap(),
            pcm_limits(),
            ChannelPolicy::Exact,
            &WavDecoder,
            AssetPathPolicy::Exact,
            seed,
            None,
        )
        .unwrap();
        assert_eq!(prepared.bank.len(), 1);
        assert_eq!(prepared.sounds[0].sample, selected_sample);
        assert_eq!(
            prepared.bank.get(selected_sample).unwrap().samples(),
            &[0.25]
        );
        assert_eq!(prepared.source.notes, expected.notes);
    }
}

fn capture_setup(text: &str) -> (ReplayFile, ReplayCodecLimits) {
    let chart = parse(text, Default::default()).unwrap();
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::ZERO,
    )
    .unwrap();
    let judge = JudgeEngine::new(chart.compile().unwrap().chart, chart.rules(), profile).unwrap();
    let limits =
        ReplayCodecLimits::new(65_536, 128, 4096, CodecLimits::new(4096, 4096).unwrap()).unwrap();
    (
        LiveReplayCapture::new(&judge, ClockDomainId(17), limits)
            .unwrap()
            .into_file(),
        limits,
    )
}

#[test]
fn invalid_chart_and_replay_setup_reject_before_any_asset_access() {
    let chart = "#BPM 120\n#WAV01 tap.wav\n#00111:01\n";
    let files = selected(chart.as_bytes()); // A valid setup reaches missing audio lookup.
    let memory = files.scope("pack/chart.bms").unwrap();
    let source = CountingSource::new(&memory);
    for invalid in [b"#BPM 0\n".as_slice(), b"\xef\xbb\xbf\x93\xfa".as_slice()] {
        assert!(prepare(invalid, &source, ChannelPolicy::Exact, 1, &WavDecoder).is_err());
        assert_eq!(source.counts(), (0, 0));
    }
    let (file, limits) = capture_setup(chart);
    let wrong_chart = chart.replace("120", "150");
    assert!(
        prepare_from_source(
            wrong_chart.as_bytes(),
            &source,
            AudioFormat::new(48_000, 1).unwrap(),
            pcm_limits(),
            ChannelPolicy::Exact,
            &WavDecoder,
            AssetPathPolicy::Exact,
            0,
            Some((&file, limits)),
        )
        .is_err()
    );
    assert_eq!(source.counts(), (0, 0));
    let mut malformed = file.clone();
    malformed.header.options = vec![];
    assert!(
        prepare_from_source(
            chart.as_bytes(),
            &source,
            AudioFormat::new(48_000, 1).unwrap(),
            pcm_limits(),
            ChannelPolicy::Exact,
            &WavDecoder,
            AssetPathPolicy::Exact,
            0,
            Some((&malformed, limits)),
        )
        .is_err()
    );
    assert_eq!(source.counts(), (0, 0));
    assert!(
        prepare_from_source(
            chart.as_bytes(),
            &source,
            AudioFormat::new(48_000, 1).unwrap(),
            pcm_limits(),
            ChannelPolicy::Exact,
            &WavDecoder,
            AssetPathPolicy::Exact,
            0,
            Some((&file, limits)),
        )
        .is_err()
    );
    assert_eq!(source.counts(), (1, 0));

    // Raw Shift-JIS names must reach the same Unicode source keys without replacement decoding.
    let shift_jis = b"#BPM 120\n#WAV01 \x93\xfa.wav\n#00011:01\n";
    let mut files = selected(shift_jis);
    files.insert("pack/日.wav", wav(48_000, &[16_384])).unwrap();
    let source = files.scope("pack/chart.bms").unwrap();
    let prepared = prepare(shift_jis, &source, ChannelPolicy::Exact, 1, &WavDecoder).unwrap();
    assert_eq!(prepared.source.samples.get(&1).unwrap(), "日.wav");
    assert_eq!(prepared.bank.get(SampleId(1)).unwrap().samples(), &[0.5]);
}

#[test]
fn image_aliases_share_raw_and_layer_bytes_and_keep_missing_reasons() {
    let text = "#BMP01 black.bmp\n#BMP02 ./black.bmp\n#BMP03 missing.bmp\n#BMP04 movie.mpg\n#00004:0102030405\n#00007:0102\n";
    let chart = parse(text, Default::default()).unwrap();
    let mut files = selected(text.as_bytes());
    files.insert("pack/black.bmp", bmp([0, 0, 0])).unwrap();
    files.insert("pack/missing.PNG", bmp([255, 0, 0])).unwrap();
    files
        .insert("pack/movie.mpg", b"unsupported movie".to_vec())
        .unwrap();
    let memory = files.scope("pack/chart.bms").unwrap();
    let source = CountingSource::new(&memory);
    let images = ImageAssets::prepare_from_source(&source, &chart, Default::default()).unwrap();
    let raw = images.get(ImageId(1)).unwrap();
    let layer = images.get_layer(ImageId(1)).unwrap();
    assert!(Arc::ptr_eq(raw, images.get(ImageId(2)).unwrap()));
    assert!(Arc::ptr_eq(layer, images.get_layer(ImageId(2)).unwrap()));
    assert!(!Arc::ptr_eq(raw, layer));
    assert_eq!(raw.pixels(), &[0, 0, 0, 255]);
    assert_eq!(layer.pixels(), &[0, 0, 0, 0]);
    assert_eq!((images.unique_images(), images.decoded_bytes()), (1, 8));
    assert_eq!(
        images.unavailable(ImageId(3)),
        Some(&ImageUnavailable::Missing)
    );
    assert_eq!(
        images.unavailable(ImageId(4)),
        Some(&ImageUnavailable::Unsupported)
    );
    assert_eq!(
        images.unavailable(ImageId(5)),
        Some(&ImageUnavailable::Undefined)
    );
    assert!(images.get(ImageId(3)).is_none());
    assert!(
        ImageAssets::prepare_from_source(
            &memory,
            &chart,
            ImageAssetLimits {
                max_decoded_bytes: 7,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn exact_image_alias_crop_canvas_and_resource_limits_apply_to_memory_sources() {
    let text = "#BMP01 red.bmp\n#BMP02 ./red.bmp\n#BGA03 01 0 0 1 1 0 0\n#00004:010203\n#00007:02\n#CANVASSIZE 3 2\n";
    let chart = parse(text, Default::default()).unwrap();
    let mut files = selected(text.as_bytes());
    files.insert("pack/red.bmp", bmp([200, 40, 20])).unwrap();
    let memory = files.scope("pack/chart.bms").unwrap();
    let source = CountingSource::new(&memory);
    let limits = ImageAssetLimits {
        max_decoded_bytes: 28,
        ..Default::default()
    };
    let images = ImageAssets::prepare_from_source(&source, &chart, limits).unwrap();
    let canvas = images.get(ImageId(1)).unwrap();
    assert_eq!((canvas.width(), canvas.height()), (3, 2));
    assert_eq!(&canvas.pixels()[..4], &[200, 40, 20, 255]);
    assert!(canvas.pixels()[4..].iter().all(|value| *value == 0));
    assert!(Arc::ptr_eq(canvas, images.get(ImageId(2)).unwrap()));
    assert!(Arc::ptr_eq(canvas, images.get(ImageId(3)).unwrap()));
    assert!(Arc::ptr_eq(canvas, images.get_layer(ImageId(2)).unwrap()));
    assert_eq!((images.unique_images(), images.decoded_bytes()), (1, 28));
    assert_eq!(source.reads.get(), 1);
    assert!(
        ImageAssets::prepare_from_source(
            &memory,
            &chart,
            ImageAssetLimits {
                max_decoded_bytes: 27,
                ..limits
            }
        )
        .is_err()
    );
    let count_limited = CountingSource::new(&memory);
    assert!(
        ImageAssets::prepare_from_source(
            &count_limited,
            &chart,
            ImageAssetLimits {
                max_images: 2,
                ..limits
            }
        )
        .is_err()
    );
    assert_eq!(count_limited.counts(), (0, 0));
    let mut encoded_limited = limits;
    encoded_limited.decode.max_encoded_bytes = 57;
    assert!(ImageAssets::prepare_from_source(&memory, &chart, encoded_limited).is_err());
    let unsafe_chart = parse("#BMP01 ../outside.bmp\n#00004:01\n", Default::default()).unwrap();
    assert!(ImageAssets::prepare_from_source(&memory, &unsafe_chart, limits).is_err());
}
