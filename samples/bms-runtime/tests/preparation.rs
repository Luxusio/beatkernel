//! Authored filesystem/PCM preparation fixtures; no native playback required.
use beatkernel::{audio::*, judge::JudgeStage};
use beatkernel_bms_runtime::*;
use std::{
    error::Error,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..1000 {
            let path = std::env::temp_dir().join(format!(
                "beatkernel-preparation-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("temporary directory: {error}"),
            }
        }
        panic!("temporary directory attempts exhausted")
    }
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn limits() -> PcmLimits {
    PcmLimits::new(1024, 4096, 8).unwrap()
}
fn wav(channels: u16, samples: &[i16]) -> Vec<u8> {
    let bytes = u32::try_from(samples.len() * 2).unwrap();
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&24_000u32.to_le_bytes());
    out.extend_from_slice(&(24_000u32 * u32::from(channels) * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&bytes.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

fn captured_setup_identity(prepared: &PreparedBms) -> beatkernel::replay::ReplayHeader {
    let profile = beatkernel::judge::JudgeProfile::new(
        vec![beatkernel::judge::JudgeWindow {
            grade: beatkernel::judge::JudgeGrade(7),
            early: beatkernel::time::Duration::ZERO,
            late: beatkernel::time::Duration::ZERO,
        }],
        beatkernel::time::Duration::ZERO,
    )
    .unwrap();
    let judge = beatkernel::judge::JudgeEngine::new(
        prepared.compiled.chart.clone(),
        prepared.source.rules(),
        profile,
    )
    .unwrap();
    replay_capture::LiveReplayCapture::new(
        &judge,
        beatkernel::time::ClockDomainId(17),
        competition_live::replay_limits().unwrap(),
    )
    .unwrap()
    .into_file()
    .header
}

#[path = "../src/flac_fixture.rs"]
mod flac_fixture;

#[path = "../src/vorbis_fixture.rs"]
#[allow(dead_code)] // Decoder-only corruption helpers are shared with unit fixtures.
mod vorbis_fixture;

#[path = "../src/mp3_fixture.rs"]
#[allow(dead_code)] // Unit codec fixtures also use frame/tag mutation helpers.
mod mp3_fixture;

#[test]
fn default_tagged_mp3_preserves_prepared_timing_identity_and_actual_rendered_pcm() {
    let dir = Directory::new();
    std::fs::create_dir(dir.0.join("assets")).unwrap();
    let encoded = mp3_fixture::tagged_silence(1, 4, 576, 1000);
    // MPEG2: 4×576 audio frames, minus 576 encoder delay and 1000 padding.
    // Decoder latency redistributes 529 frames between the leading/trailing cuts.
    let retained = 728usize;
    dir.write("assets/日本.mP3", &encoded);
    dir.write("head.wav", &wav(1, &[16384, -8192]));
    let path = dir.write(
        "chart.bms",
        "#BPM 60\n#WAV01 head.wav\n#WAV02 assets\\日本.wav\n#00011:01\n#00001:02\n".as_bytes(),
    );
    let format = AudioFormat::new(24_000, 2).unwrap();
    let pcm_limits = PcmLimits::new(retained * 2 * 4, 8192, 8).unwrap();
    let prepared = load_prepared(&path, format, pcm_limits, ChannelPolicy::MonoToStereo).unwrap();
    assert_eq!(prepared.source.samples[&2], "assets\\日本.wav");
    let bgm = prepared.bank.get(SampleId(2)).unwrap();
    assert_eq!(bgm.frames(), retained);
    assert_eq!(bgm.samples(), vec![0.0; retained * 2]);
    let identity = captured_setup_identity(&prepared);
    assert!(
        load_prepared_with_decoder(
            &path,
            format,
            pcm_limits,
            ChannelPolicy::MonoToStereo,
            &DefaultAssetDecoder
        )
        .is_err()
    );
    let raw = mp3_decode::Mp3Decoder
        .decode_with_timing(
            Path::new("not-opened.mp3"),
            &encoded,
            PcmLimits::new(9216, 9216, 1).unwrap(),
            mp3_decode::Mp3TimingPolicy::RawFrames,
        )
        .unwrap();
    assert_eq!(raw.frames(), 2304);
    dir.write("assets/日本.wav", &wav(1, &vec![0; retained]));
    let literal = load_prepared(&path, format, pcm_limits, ChannelPolicy::MonoToStereo).unwrap();
    assert_eq!(captured_setup_identity(&literal), identity);
    let mut results = Vec::new();
    for chart in [prepared, literal] {
        let mut output = Vec::new();
        let report = offline::render_offline(
            chart,
            offline::OfflineOptions {
                frames: u64::try_from(retained + 1).unwrap(),
                block_frames: 31,
                command_capacity: 4,
                max_voices: 2,
            },
            &mut output,
        )
        .unwrap();
        assert_eq!(report.hits, 1);
        let mut expected = vec![0.0f32; (retained + 1) * 2];
        expected[..4].copy_from_slice(&[0.5, 0.5, -0.25, -0.25]);
        assert_eq!(
            output,
            expected
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect::<Vec<_>>()
        );
        results.push(output);
    }
    assert_eq!(results[0], results[1]);
    dir.write("assets/日本.wav", &encoded); // Content dispatch ignores filename suffix.
    assert_eq!(
        load_prepared(&path, format, pcm_limits, ChannelPolicy::MonoToStereo)
            .unwrap()
            .bank
            .get(SampleId(2))
            .unwrap()
            .frames(),
        retained
    );
    let small = PcmLimits::new(retained * 8 - 1, 8192, 8).unwrap();
    assert!(load_prepared(&path, format, small, ChannelPolicy::MonoToStereo).is_err());
    dir.write("assets/日本.wav", b"ID3damaged");
    assert!(load_prepared(&path, format, pcm_limits, ChannelPolicy::MonoToStereo).is_err());
}

#[test]
fn default_vorbis_reference_variants_preserve_pcm_extent_and_literal_errors() {
    let dir = Directory::new();
    std::fs::create_dir(dir.0.join("assets")).unwrap();
    let encoded = vorbis_fixture::packed_audio(1, 48);
    dir.write("assets/日本.OgG", &encoded);
    dir.write("head.wav", &wav(1, &[16384, -8192]));
    let path = dir.write(
        "chart.bms",
        "#BPM 60\n#WAV01 head.wav\n#WAV02 assets\\日本.wav\n#00011:01\n#00001:02\n".as_bytes(),
    );
    let format = AudioFormat::new(24_000, 2).unwrap();
    let prepare = || load_prepared(&path, format, limits(), ChannelPolicy::MonoToStereo);
    let prepared = prepare().unwrap();
    assert_eq!(prepared.source.samples[&2], "assets\\日本.wav");
    let bgm = prepared.bank.get(SampleId(2)).unwrap();
    assert_eq!(bgm.format(), format);
    assert_eq!(bgm.frames(), 48);
    assert_eq!(bgm.samples(), &[0.0; 96]);
    let mut output = Vec::new();
    let report = offline::render_offline(
        prepared,
        offline::OfflineOptions {
            frames: 49,
            block_frames: 7,
            command_capacity: 4,
            max_voices: 2,
        },
        &mut output,
    )
    .unwrap();
    assert_eq!(report.hits, 1);
    let mut expected = vec![0.0f32; 98];
    expected[..4].copy_from_slice(&[0.5, 0.5, -0.25, -0.25]);
    assert_eq!(
        output,
        expected
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>()
    );
    assert!(
        load_prepared_with_decoder(
            &path,
            format,
            limits(),
            ChannelPolicy::MonoToStereo,
            &DefaultAssetDecoder
        )
        .is_err()
    );
    // Content selects Vorbis even under the literal WAV reference.
    dir.write("assets/日本.wav", &encoded);
    assert_eq!(
        prepare().unwrap().bank.get(SampleId(2)).unwrap().frames(),
        48
    );
    dir.write("assets/日本.wav", b"OggSdamaged");
    assert!(prepare().is_err()); // A valid alternate never conceals a damaged literal.
    // Ogg is not silently passed to the strict WAV-only codec.
    assert!(
        WavDecoder
            .decode(Path::new("test.wav"), &encoded, limits())
            .is_err()
    );
    let cap = PcmLimits::new(383, 4096, 8).unwrap();
    dir.write("assets/日本.wav", &encoded);
    assert!(load_prepared(&path, format, cap, ChannelPolicy::MonoToStereo).is_err());
    let exact_cap = PcmLimits::new(384, 4096, 8).unwrap();
    assert!(load_prepared(&path, format, exact_cap, ChannelPolicy::MonoToStereo).is_ok());
}

#[test]
fn converted_mixed_case_flac_default_lookup_preserves_chart_identity_and_rendered_pcm() {
    let dir = Directory::new();
    std::fs::create_dir(dir.0.join("assets")).unwrap();
    dir.write(
        "assets/日本.FlAc",
        &flac_fixture::flac16(24_000, 1, &[16384, -8192], Some(2)),
    );
    let path = dir.write(
        "chart.bms",
        "#BPM 60\n#WAV01 assets\\日本.wav\n#00011:01\n".as_bytes(),
    );
    let format = AudioFormat::new(24_000, 2).unwrap();
    assert!(
        load_prepared_with_decoder(
            &path,
            format,
            limits(),
            ChannelPolicy::MonoToStereo,
            &DefaultAssetDecoder
        )
        .is_err()
    ); // Existing custom-codec entry stays Exact.
    assert!(
        load_prepared_with_decoder_and_paths(
            &path,
            format,
            limits(),
            ChannelPolicy::MonoToStereo,
            &DefaultAssetDecoder,
            asset_paths::AssetPathPolicy::Exact
        )
        .is_err()
    );
    let converted = load_prepared(&path, format, limits(), ChannelPolicy::MonoToStereo).unwrap();
    assert_eq!(converted.source.samples[&1], "assets\\日本.wav");
    assert_eq!(
        converted.bank.get(SampleId(1)).unwrap().samples(),
        &[0.5, 0.5, -0.25, -0.25]
    );
    let original_identity = captured_setup_identity(&converted);
    dir.write("assets/日本.wav", &wav(1, &[16384, -8192]));
    let exact = load_prepared(&path, format, limits(), ChannelPolicy::MonoToStereo).unwrap();
    assert_eq!(captured_setup_identity(&exact), original_identity);
    let mut results = Vec::new();
    for prepared in [converted, exact] {
        let mut bytes = Vec::new();
        let report = offline::render_offline(
            prepared,
            offline::OfflineOptions {
                frames: 2,
                block_frames: 1,
                command_capacity: 4,
                max_voices: 1,
            },
            &mut bytes,
        )
        .unwrap();
        assert_eq!(report.hits, 1);
        let samples: Vec<_> = bytes
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        assert_eq!(samples, [0.5, 0.5, -0.25, -0.25]);
        results.push(bytes);
    }
    assert_eq!(results[0], results[1]);
    dir.write("assets/日本.wav", b"damaged original");
    assert!(load_prepared(&path, format, limits(), ChannelPolicy::MonoToStereo).is_err());
}

#[test]
fn default_flac_and_wav_assets_feed_actual_runtime_mixer_without_tail_loading() {
    let dir = Directory::new();
    dir.write("head.wav", &wav(1, &[16384, -8192]));
    let encoded = flac_fixture::flac16(24_000, 1, &[8192, -16384], Some(2));
    dir.write("music.flac", &encoded);
    let chart = dir.write("chart.bms", b"#BPM 60\n#LNOBJ ZZ\n#WAV01 head.wav\n#WAV02 music.flac\n#WAV03 unused.flac\n#00011:01ZZ\n#00001:02\n");
    let format = AudioFormat::new(24_000, 2).unwrap();
    let prepared = load_prepared(&chart, format, limits(), ChannelPolicy::MonoToStereo).unwrap();
    assert_eq!(prepared.bank.len(), 2); // Unused samples and undefined tail ZZ are never opened.
    assert_eq!(prepared.bank.get(SampleId(2)).unwrap().format(), format);
    assert_eq!(
        prepared.bank.get(SampleId(2)).unwrap().samples(),
        &[0.25, 0.25, -0.5, -0.5]
    );
    assert_eq!(prepared.sounds.len(), 1);
    assert_eq!(prepared.sounds[0].stage, JudgeStage::HoldHead);
    assert_eq!(prepared.bgm_commands.len(), 1);
    let mut output = Vec::new();
    let report = offline::render_offline(
        prepared,
        offline::OfflineOptions {
            frames: 2,
            block_frames: 1,
            command_capacity: 4,
            max_voices: 2,
        },
        &mut output,
    )
    .unwrap();
    assert_eq!(report.frames, 2);
    assert_eq!(report.hits, 1);
    let samples: Vec<_> = output
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect();
    assert_eq!(samples, [0.75, 0.75, -0.75, -0.75]);
    // Dispatch follows bytes even when a caller's path suffix describes WAV.
    let pcm = DefaultAssetDecoder
        .decode(Path::new("misleading.wav"), &encoded, limits())
        .unwrap();
    assert_eq!(pcm.format(), AudioFormat::new(24_000, 1).unwrap());
    assert_eq!(pcm.samples(), &[0.25, -0.5]);
    assert!(
        WavDecoder
            .decode(Path::new("music.flac"), &encoded, limits())
            .is_err()
    );
    let mut changed = encoded.clone();
    changed[45] = 0x0c; // Valid frame CRC with a conflicting 24-bit frame header.
    flac_fixture::refresh_flac16_checksums(&mut changed);
    dir.write("music.flac", &changed);
    assert!(load_prepared(&chart, format, limits(), ChannelPolicy::MonoToStereo).is_err());
    let mut broken = encoded;
    broken.pop();
    dir.write("music.flac", &broken);
    assert!(load_prepared(&chart, format, limits(), ChannelPolicy::MonoToStereo).is_err());
}

#[test]
fn shift_jis_catalog_and_asset_loaders_preserve_unicode_paths_and_pcm() {
    let dir = Directory::new();
    dir.write("日本.wav", &wav(1, &[16384, -8192]));
    let utf8 = "#TITLE 日本\n#ARTIST 日本\n#BPM 60\n#LNOBJ ZZ\n#WAV01 日本.wav\n#00011:01ZZ\n";
    let mut legacy = Vec::new();
    for (index, ascii) in utf8.split("日本").enumerate() {
        if index != 0 {
            legacy.extend_from_slice(&[0x93, 0xfa, 0x96, 0x7b]);
        }
        legacy.extend_from_slice(ascii.as_bytes());
    }
    let path = dir.write("chart.bms", &legacy);
    let source = competition_live::load_chart(&path).unwrap();
    assert_eq!(source.metadata["TITLE"], "日本");
    assert_eq!(source.metadata["ARTIST"], "日本");
    let library = player_chart::scan_library(&dir.0).unwrap();
    assert!(library.diagnostics.is_empty());
    assert_eq!(library.entries.len(), 1);
    assert_eq!(library.entries[0].path, path);
    assert_eq!(library.entries[0].title, "日本");
    assert_eq!(library.entries[0].artist, "日本");
    let format = AudioFormat::new(48_000, 2).unwrap();
    let prepared = load_prepared(&path, format, limits(), ChannelPolicy::MonoToStereo).unwrap();
    assert_eq!(prepared.bank.len(), 1);
    assert_eq!(
        prepared.bank.get(SampleId(1)).unwrap().samples(),
        &[0.5, 0.5, -0.25, -0.25]
    );
    assert_eq!(prepared.source.samples[&1], "日本.wav");
    let utf8_path = dir.write("utf8.bms", utf8.as_bytes());
    let reference =
        load_prepared(&utf8_path, format, limits(), ChannelPolicy::MonoToStereo).unwrap();
    let profile = beatkernel::judge::JudgeProfile::new(
        vec![beatkernel::judge::JudgeWindow {
            grade: beatkernel::judge::JudgeGrade(7),
            early: beatkernel::time::Duration::ZERO,
            late: beatkernel::time::Duration::ZERO,
        }],
        beatkernel::time::Duration::ZERO,
    )
    .unwrap();
    let actual = beatkernel::judge::JudgeEngine::new(
        prepared.compiled.chart,
        prepared.source.rules(),
        profile.clone(),
    )
    .unwrap();
    let expected = beatkernel::judge::JudgeEngine::new(
        reference.compiled.chart,
        reference.source.rules(),
        profile,
    )
    .unwrap();
    assert_eq!(
        actual.stable_hash().unwrap(),
        expected.stable_hash().unwrap()
    );
}

#[test]
fn real_wav_bpm_stop_hold_bgm_and_explicit_mono_expansion() {
    let dir = Directory::new();
    dir.write("head.wav", &wav(1, &[16384, -8192]));
    dir.write("bgm.wav", &wav(2, &[8192, -16384]));
    let chart = dir.write("chart.bms", b"#BPM 60\n#BPM01 120\n#STOP01 48\n#LNTYPE 1\n#WAV01 head.wav\n#WAV02 bgm.wav\n#WAV03 missing-unused.wav\n#00008:0001\n#00009:0001\n#00051:0104\n#00112:01\n#00001:0002\n#00001:0002\n");
    let prepared = load_prepared(
        &chart,
        AudioFormat::new(48_000, 2).unwrap(),
        limits(),
        ChannelPolicy::MonoToStereo,
    )
    .unwrap();
    assert_eq!(prepared.bank.len(), 2); // Undefined tail 04 and unused 03 are not loaded.
    assert_eq!(prepared.bank.total_bytes(), 24);
    let head = prepared.bank.get(SampleId(1)).unwrap();
    assert_eq!(head.format(), AudioFormat::new(24_000, 2).unwrap());
    assert_eq!(head.samples(), &[0.5, 0.5, -0.25, -0.25]);
    assert_eq!(
        prepared.bank.get(SampleId(2)).unwrap().samples(),
        &[0.25, -0.5]
    );
    assert_eq!(prepared.bank.format(), AudioFormat::new(48_000, 2).unwrap());
    let hold = prepared
        .compiled
        .chart
        .objects()
        .iter()
        .find(|object| object.time.end.is_some())
        .unwrap();
    assert_eq!(hold.time.start.as_nanos(), 0);
    assert_eq!(hold.time.end.unwrap().as_nanos(), 2_000_000_000);
    let instant = prepared
        .compiled
        .chart
        .objects()
        .iter()
        .find(|object| object.time.end.is_none())
        .unwrap();
    assert_eq!(instant.time.start.as_nanos(), 3_500_000_000);
    assert!(
        prepared
            .sounds
            .iter()
            .any(|sound| sound.object == hold.id && sound.stage == JudgeStage::HoldHead)
    );
    assert!(
        prepared
            .sounds
            .iter()
            .all(|sound| sound.voice == VoiceId(sound.object.0))
    );
    assert_eq!(prepared.bgm_commands.len(), 2);
    let maximum = prepared
        .compiled
        .chart
        .objects()
        .iter()
        .map(|object| object.id.0)
        .max()
        .unwrap();
    for (index, command) in prepared.bgm_commands.iter().enumerate() {
        match command {
            AudioCommand::Play {
                voice,
                sample,
                at,
                gain,
            } => {
                assert_eq!(*voice, VoiceId(maximum + 1 + index as u64));
                assert_eq!(*sample, SampleId(2));
                assert_eq!(at.as_nanos(), 2_000_000_000); // BGM receives pre-STOP time.
                assert_eq!(*gain, 1.0);
            }
            _ => panic!("preparation emitted a non-Play BGM command"),
        }
    }
    assert!(
        load_prepared(
            &chart,
            AudioFormat::new(48_000, 2).unwrap(),
            limits(),
            ChannelPolicy::Exact
        )
        .is_err()
    );
    assert!(
        load_prepared(
            &chart,
            AudioFormat::new(48_000, 3).unwrap(),
            limits(),
            ChannelPolicy::MonoToStereo
        )
        .is_err()
    );
    assert!(
        load_prepared(
            &chart,
            AudioFormat::new(48_000, 2).unwrap(),
            PcmLimits::new(8, 64, 8).unwrap(),
            ChannelPolicy::MonoToStereo
        )
        .is_err()
    );
}

#[test]
fn exact_channels_empty_chart_and_malformed_wav_are_explicit() {
    let dir = Directory::new();
    dir.write("mono.wav", &wav(1, &[i16::MIN, i16::MAX]));
    let chart = dir.write("mono.bms", b"#WAV01 mono.wav\n#00011:01");
    let prepared = load_prepared(
        &chart,
        AudioFormat::new(44_100, 1).unwrap(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert_eq!(
        prepared.bank.get(SampleId(1)).unwrap().samples(),
        &[-1.0, 32767.0 / 32768.0]
    );
    let empty = dir.write("empty.bms", b"#BPM 120\n#WAV01 nonexistent.wav");
    let prepared = load_prepared(
        &empty,
        AudioFormat::new(96_000, 2).unwrap(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert!(prepared.bank.is_empty());
    assert!(prepared.sounds.is_empty());
    assert!(prepared.bgm_commands.is_empty());
    assert_eq!(prepared.bank.format(), AudioFormat::new(96_000, 2).unwrap());
    dir.write("mono.wav", b"not RIFF WAVE");
    assert!(
        load_prepared(
            &chart,
            AudioFormat::new(44_100, 1).unwrap(),
            limits(),
            ChannelPolicy::Exact
        )
        .is_err()
    );
}

struct TestDecoder;
impl AssetDecoder for TestDecoder {
    fn decode(
        &self,
        path: &Path,
        encoded: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>> {
        assert!(path.is_absolute());
        assert_eq!(encoded, b"caller codec bytes");
        Ok(PcmSample::new(
            AudioFormat::new(32_000, 1)?,
            vec![1.25, -2.0],
            limits,
        )?)
    }
}
#[test]
fn explicit_custom_decoder_receives_contained_bytes_and_keeps_amplitudes() {
    let dir = Directory::new();
    std::fs::create_dir(dir.0.join("assets")).unwrap();
    dir.write("assets/custom.bin", b"caller codec bytes");
    let chart = dir.write("chart.bms", b"#WAV01 assets\\custom.bin\n#00011:01");
    let prepared = load_prepared_with_decoder(
        &chart,
        AudioFormat::new(48_000, 2).unwrap(),
        limits(),
        ChannelPolicy::MonoToStereo,
        &TestDecoder,
    )
    .unwrap();
    assert_eq!(
        prepared.bank.get(SampleId(1)).unwrap().samples(),
        &[1.25, 1.25, -2.0, -2.0]
    );
    assert!(
        load_prepared(
            &chart,
            AudioFormat::new(48_000, 2).unwrap(),
            limits(),
            ChannelPolicy::MonoToStereo
        )
        .is_err()
    );
}

#[test]
fn portable_absolute_parent_drive_and_symlink_escape_names_reject() {
    let dir = Directory::new();
    for name in [
        "../outside.wav",
        "..\\outside.wav",
        "/outside.wav",
        "\\outside.wav",
        "C:\\outside.wav",
        "C:outside.wav",
        "\\\\server\\share.wav",
    ] {
        let chart = dir.write("chart.bms", format!("#WAV01 {name}\n#00011:01").as_bytes());
        assert!(
            load_prepared(
                &chart,
                AudioFormat::new(48_000, 1).unwrap(),
                limits(),
                ChannelPolicy::Exact
            )
            .is_err(),
            "{name}"
        );
    }
    #[cfg(unix)]
    {
        let outside = Directory::new();
        let asset = outside.write("outside.wav", &wav(1, &[0]));
        std::os::unix::fs::symlink(asset, dir.0.join("escaped.wav")).unwrap();
        let chart = dir.write("chart.bms", b"#WAV01 escaped.wav\n#00011:01");
        assert!(
            load_prepared(
                &chart,
                AudioFormat::new(48_000, 1).unwrap(),
                limits(),
                ChannelPolicy::Exact
            )
            .is_err()
        );
    }
}

#[test]
fn chart_note_count_is_independent_of_active_voice_capacity() {
    let dir = Directory::new();
    dir.write("note.wav", &wav(1, &[0]));
    let mut text = String::from("#BPM 120\n#WAV01 note.wav\n");
    for measure in 0..5 {
        text.push_str(&format!("#{measure:03}11:{}\n", "01".repeat(1000)));
    }
    text.push_str("#00401:01\n");
    let chart = dir.write("many.bms", text.as_bytes());
    let prepared = load_prepared(
        &chart,
        AudioFormat::new(48_000, 1).unwrap(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert_eq!(prepared.sounds.len(), 5000);
    assert_eq!(prepared.bank.len(), 1);
    assert_eq!(
        prepared
            .sounds
            .iter()
            .map(|sound| sound.voice.0)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        5000
    );
    match prepared.bgm_commands[0] {
        AudioCommand::Play { voice, .. } => assert_eq!(voice, VoiceId(5001)),
        _ => panic!("unexpected BGM command"),
    }
}

#[test]
fn seeded_branches_share_loading_pcm_and_replay_setup() {
    let dir = Directory::new();
    dir.write("second.wav", &wav(1, &[16384, -8192]));
    let path = dir.write("random.bms", b"#BPM 60\n#RANDOM 2\n#IF 1\n#WAV01 first.wav\n#00011:01\n#ELSE\n#WAV02 second.wav\n#00012:02\n#00001:02\n#ENDIF\n");
    let format = AudioFormat::new(24_000, 1).unwrap();
    let baseline = load_prepared(&path, format, limits(), ChannelPolicy::Exact).unwrap();
    assert_eq!(baseline.source.notes.len(), 1);
    assert_eq!(baseline.source.notes[0].sample, SampleId(2));
    assert!(!baseline.source.samples.contains_key(&1));
    assert!(baseline.bank.get(SampleId(1)).is_none());
    assert_eq!(
        baseline.bank.get(SampleId(2)).unwrap().samples(),
        &[0.5, -0.25]
    );
    let loaded = competition_live::load_chart_with_seed(&path, 0).unwrap();
    assert_eq!(loaded.source, baseline.source.source);
    assert_eq!(
        competition_live::load_chart(&path).unwrap().source,
        loaded.source
    );
    let again = load_prepared_with_seed(&path, format, limits(), ChannelPolicy::Exact, 0).unwrap();
    let identity = captured_setup_identity(&baseline);
    assert_eq!(captured_setup_identity(&again), identity);
    let recording = beatkernel::replay::codec::ReplayFile::new(identity.clone(), Vec::new());
    let replay_limits = competition_live::replay_limits().unwrap();
    replay_playback::validate_setup(&loaded, &recording, replay_limits).unwrap();
    let mut replay =
        replay_playback::reconstruct(&loaded, recording.clone(), replay_limits).unwrap();
    replay.seek(beatkernel::time::Timestamp::ZERO).unwrap();
    // Seed3 selects the missing first asset; its branch cannot silently use second.wav.
    assert!(load_prepared_with_seed(&path, format, limits(), ChannelPolicy::Exact, 3).is_err());
    dir.write("first.wav", &wav(1, &[8192, 4096]));
    let other = load_prepared_with_seed(&path, format, limits(), ChannelPolicy::Exact, 3).unwrap();
    assert_eq!(other.source.notes[0].sample, SampleId(1));
    assert_ne!(
        captured_setup_identity(&other).chart_identity,
        identity.chart_identity
    );
    assert!(replay_playback::validate_setup(&other.source, &recording, replay_limits).is_err());
    assert!(replay_playback::reconstruct(&other.source, recording, replay_limits).is_err());
    let mut output = Vec::new();
    let report = offline::render_offline(
        baseline,
        offline::OfflineOptions {
            frames: 3,
            block_frames: 1,
            command_capacity: 4,
            max_voices: 2,
        },
        &mut output,
    )
    .unwrap();
    assert_eq!(report.hits, 1);
    assert_eq!(
        output,
        [1.0f32, -0.5, 0.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>()
    );
}

#[test]
fn durable_source_seed_restores_real_capture_assets_pcm_and_replay() {
    use beatkernel::{
        input::{
            Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
            PhysicalControlId, PhysicalInputEvent,
        },
        judge::JudgeEngine,
        replay::codec::{decode_replay, encode_replay},
        runtime::Runtime,
        time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
        transport::{Rate, Transport},
    };
    struct Identity;
    impl ClockMapper for Identity {
        fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
            (from.domain == to).then_some(from.timestamp)
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Exact
        }
    }
    let point = |nanos| ClockPoint {
        domain: ClockDomainId(17),
        timestamp: Timestamp::from_nanos(nanos),
    };
    let dir = Directory::new();
    dir.write("first.wav", &wav(1, &[16384, -8192]));
    let path = dir.write("seed.bms", b"#BPM 60\n#RANDOM 2\n#IF 1\n#WAV01 first.wav\n#00011:01\n#00001:01\n#ELSE\n#WAV02 missing.wav\n#00012:02\n#00001:02\n#ENDIF\n");
    let format = AudioFormat::new(24_000, 1).unwrap();
    assert!(load_prepared(&path, format, limits(), ChannelPolicy::Exact).is_err());
    let live = load_prepared_with_seed(&path, format, limits(), ChannelPolicy::Exact, 3).unwrap();
    let profile = replay_playback::decode_profile(&captured_setup_identity(&live).options).unwrap();
    let judge =
        JudgeEngine::new(live.compiled.chart.clone(), live.source.rules(), profile).unwrap();
    let replay_limits = competition_live::replay_limits().unwrap();
    let mut capture = replay_capture::LiveReplayCapture::new_at_with_chart_seed(
        &judge,
        ClockDomainId(17),
        replay_limits,
        Timestamp::ZERO,
        3,
    )
    .unwrap();
    let control = live.source.notes[0].lane.control();
    let physical = PhysicalControlId::keyboard(7);
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Exact(DeviceId(3)),
        physical,
        game_control: control,
    }])
    .unwrap();
    let (producer, _consumer) = command_queue(1).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(17),
        ClockDomainId(17),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    let report = runtime
        .process_input(
            PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(DeviceId(3), point(0), 0),
                control: physical,
                state: ButtonState::Down,
            }),
            &Identity,
            point(0),
        )
        .unwrap();
    assert!(report.judge_error.is_none());
    assert_eq!(report.judge_events.len(), 1);
    let mut live_pressed = pressed_keys::PressedKeys::default();
    live_pressed.apply(&report.bound_inputs).unwrap();
    assert_eq!(live_pressed.mask(), 1); // Actual admitted BMS 11 button.
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_chart(&live.source, &live.compiled.chart)
            .map_err(|error| error.to_string())?;
        viewer.take_latest();
        player::publish_report(&report).map_err(|error| error.to_string())?;
        player::publish_pause(player::PauseState::Running); // Force actual state handoff.
        let shown = viewer.take_latest().unwrap();
        assert_eq!(shown.pressed_lanes, live_pressed.mask());
        assert_eq!(
            shown.note_progress.as_ref().unwrap().state(0),
            Some(note_progress::NoteState::Completed)
        );
        assert_eq!(
            shown.players[0].note_progress.as_ref().unwrap().state(0),
            Some(note_progress::NoteState::Completed)
        );

        assert_eq!(shown.players[0].pressed_lanes, live_pressed.mask());
        player::publish_pause(player::PauseState::Paused);
        assert_eq!(viewer.take_latest().unwrap().pressed_lanes, 0);
        player::publish_pause(player::PauseState::Running);
        assert_eq!(
            viewer.take_latest().unwrap().pressed_lanes,
            live_pressed.mask()
        );
        Ok(())
    })
    .unwrap();
    let finished = viewer.take_latest().unwrap();
    assert_eq!(finished.pressed_lanes, 0);
    assert_eq!(finished.players[0].pressed_lanes, 0);
    assert_eq!(
        finished.note_progress.as_ref().unwrap().state(0),
        Some(note_progress::NoteState::Completed)
    );

    let live_results = report.judge_events.clone();
    let display_chart =
        player_chart::PlayerChart::from_compiled(&live.source, &live.compiled.chart).unwrap();
    let initial_feedback =
        judge_feedback::project(&display_chart, report.song_time, &live_results).unwrap();
    assert_eq!(initial_feedback[0].unwrap().event, live_results[0]);
    assert_eq!(initial_feedback[0].unwrap().age_ns, 0);
    assert!(initial_feedback.iter().skip(1).all(Option::is_none));
    assert_eq!(
        judge_feedback::project(&display_chart, report.song_time, &live_results).unwrap(),
        initial_feedback
    ); // Equal reported song time preserves the pulse while paused.
    capture.record_report(&report).unwrap();
    let advanced = runtime.advance_to(point(1), &Identity, point(1)).unwrap();
    capture.record_report(&advanced).unwrap();
    live_pressed.apply(&advanced.bound_inputs).unwrap();
    assert_eq!(live_pressed.mask(), 1);
    let live_feedback =
        judge_feedback::project(&display_chart, advanced.song_time, &live_results).unwrap();
    assert_eq!(live_feedback[0].unwrap().age_ns, 1);
    let recorded = capture.into_file();
    let file = decode_replay(
        &encode_replay(&recorded, replay_limits).unwrap(),
        replay_limits,
    )
    .unwrap();
    assert_eq!(file, recorded);
    let mut visual = replay_visual::ReplayVisual::new(&live.source, &file, replay_limits).unwrap();
    assert_eq!(visual.pressed_lanes(), 0);
    visual.advance_to(Timestamp::from_nanos(-1)).unwrap();
    assert_eq!(visual.pressed_lanes(), 0);
    visual.advance_to(advanced.song_time).unwrap();
    assert_eq!(visual.pressed_lanes(), live_pressed.mask());
    let prior = visual.pressed_lanes();
    assert!(visual.advance_to(Timestamp::ZERO).is_err());
    assert_eq!(visual.pressed_lanes(), prior);
    let fresh = replay_visual::ReplayVisual::new(&live.source, &file, replay_limits).unwrap();
    assert_eq!(fresh.pressed_lanes(), 0);

    assert_eq!(file.header.seed, 0); // Judge-rule seed stays distinct.
    assert_eq!(
        replay_playback::decode_chart_setup(&file.header.options)
            .unwrap()
            .2,
        3
    );
    let restored = load_prepared_for_replay(
        &path,
        format,
        limits(),
        ChannelPolicy::Exact,
        &file,
        replay_limits,
    )
    .unwrap();
    assert_eq!(restored.source.source, live.source.source);
    assert_eq!(
        restored.bank.get(SampleId(1)).unwrap().samples(),
        &[0.5, -0.25]
    );
    assert!(restored.bank.get(SampleId(2)).is_none());
    let mut session =
        replay_playback::reconstruct(&restored.source, file.clone(), replay_limits).unwrap();
    session.seek_cursor(file.records.len()).unwrap();
    assert_eq!(session.results(), live_results);
    assert_eq!(
        judge_feedback::project(
            &display_chart,
            file.records.last().unwrap().song_time,
            session.results()
        )
        .unwrap(),
        live_feedback
    );
    assert_eq!(
        session.engine().stable_hash().unwrap(),
        runtime.judge().stable_hash().unwrap()
    );
    session.seek_cursor(0).unwrap();
    assert!(
        judge_feedback::project(&display_chart, Timestamp::ZERO, session.results())
            .unwrap()
            .iter()
            .all(Option::is_none)
    );
    session.seek_cursor(file.records.len()).unwrap();
    assert_eq!(session.results(), live_results);
    for now in [
        Timestamp::from_nanos(-1),
        Timestamp::from_nanos(judge_feedback::FEEDBACK_LIFETIME_NS),
    ] {
        assert!(
            judge_feedback::project(&display_chart, now, session.results())
                .unwrap()
                .iter()
                .all(Option::is_none)
        );
    }
    let mut output = Vec::new();
    let report = replay_render::render_replay(
        restored,
        file.clone(),
        replay_limits,
        offline::OfflineOptions {
            frames: 3,
            block_frames: 1,
            command_capacity: 4,
            max_voices: 2,
        },
        Duration::ZERO,
        &mut output,
    )
    .unwrap();
    assert_eq!(report.hits, 1);
    assert_eq!(
        output,
        [1.0f32, -0.5, 0.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    // An incompatible setup rejects before attempting the now-missing selected asset.
    std::fs::remove_file(dir.0.join("first.wav")).unwrap();
    let mut wrong = file.clone();
    wrong.header.chart_identity.push(0);
    let error = load_prepared_for_replay(
        &path,
        format,
        limits(),
        ChannelPolicy::Exact,
        &wrong,
        replay_limits,
    )
    .err()
    .unwrap();
    assert!(matches!(
        error.downcast_ref::<replay_playback::PlaybackError>(),
        Some(replay_playback::PlaybackError::IdentityMismatch(
            "compiled judge setup/profile"
        ))
    ));
    let mut malformed = file;
    malformed.header.options = b"bms-judge-profile/v3:".to_vec();
    let error = load_prepared_for_replay(
        &dir.0.join("does-not-exist.bms"),
        format,
        limits(),
        ChannelPolicy::Exact,
        &malformed,
        replay_limits,
    )
    .err()
    .unwrap();
    assert!(matches!(
        error.downcast_ref::<replay_playback::PlaybackError>(),
        Some(replay_playback::PlaybackError::Metadata(_))
    ));
}

#[test]
fn switch_fallthrough_restores_selected_assets_and_pcm_from_recorded_seed() {
    use beatkernel::{
        judge::JudgeEngine,
        replay::codec::{decode_replay, encode_replay},
        time::{ClockDomainId, Timestamp},
    };
    let dir = Directory::new();
    dir.write("head.wav", &wav(1, &[16384, -8192]));
    dir.write("bgm.wav", &wav(1, &[8192, -4096]));
    let path = dir.write(
        "switch.bms",
        b"#BPM 60\n#SWITCH 5\n#CASE 1\n#WAV01 head.wav\n#00011:01\n#CASE 2\n#WAV02 bgm.wav\n#00001:02\n#SKIP\n#CASE 3\n#WAV03 absent.wav\n#00013:03\n#DEF\n#WAV04 missing.wav\n#00012:04\n#ENDSW\n",
    );
    let format = AudioFormat::new(24_000, 1).unwrap();
    // Seed3 selects CASE1, falls through CASE2, then SKIP masks missing assets.
    // Seed0 chooses 5 and reaches DEF, whose selected asset really is missing.
    assert!(load_prepared(&path, format, limits(), ChannelPolicy::Exact).is_err());
    let prepared =
        load_prepared_with_seed(&path, format, limits(), ChannelPolicy::Exact, 3).unwrap();
    assert_eq!(prepared.source.notes.len(), 1);
    assert_eq!(prepared.source.notes[0].line, 5);
    assert_eq!(prepared.source.notes[0].sample, SampleId(1));
    assert_eq!(prepared.source.bgm.len(), 1);
    assert_eq!(prepared.source.samples.len(), 2);
    assert_eq!(
        prepared.bank.get(SampleId(1)).unwrap().samples(),
        &[0.5, -0.25]
    );
    assert_eq!(
        prepared.bank.get(SampleId(2)).unwrap().samples(),
        &[0.25, -0.125]
    );
    assert!(prepared.bank.get(SampleId(3)).is_none());
    assert!(prepared.bank.get(SampleId(4)).is_none());
    let identity = captured_setup_identity(&prepared);
    let profile = replay_playback::decode_profile(&identity.options).unwrap();
    let judge = JudgeEngine::new(
        prepared.compiled.chart.clone(),
        prepared.source.rules(),
        profile,
    )
    .unwrap();
    let replay_limits = competition_live::replay_limits().unwrap();
    let recorded = replay_capture::LiveReplayCapture::new_at_with_chart_seed(
        &judge,
        ClockDomainId(17),
        replay_limits,
        Timestamp::ZERO,
        3,
    )
    .unwrap()
    .into_file();
    let file = decode_replay(
        &encode_replay(&recorded, replay_limits).unwrap(),
        replay_limits,
    )
    .unwrap();
    assert_eq!(file, recorded);
    assert_eq!(
        replay_playback::decode_chart_setup(&file.header.options)
            .unwrap()
            .2,
        3
    );
    let restored = load_prepared_for_replay(
        &path,
        format,
        limits(),
        ChannelPolicy::Exact,
        &file,
        replay_limits,
    )
    .unwrap();
    assert_eq!(restored.source.source, prepared.source.source);
    assert_eq!(captured_setup_identity(&restored), identity);
    let selected = competition_live::load_chart_with_seed(&path, 3).unwrap();
    replay_playback::validate_setup(&selected, &file, replay_limits).unwrap();
    let default = competition_live::load_chart(&path).unwrap();
    assert_eq!(default.notes[0].sample, SampleId(4));
    assert!(replay_playback::validate_setup(&default, &file, replay_limits).is_err());
    let mut replay = replay_playback::reconstruct(&restored.source, file, replay_limits).unwrap();
    replay.seek(Timestamp::ZERO).unwrap();
    // The ordinary runtime and mixer must sum precisely the selected key and BGM.
    for prepared in [prepared, restored] {
        let mut output = Vec::new();
        let report = offline::render_offline(
            prepared,
            offline::OfflineOptions {
                frames: 3,
                block_frames: 1,
                command_capacity: 4,
                max_voices: 2,
            },
            &mut output,
        )
        .unwrap();
        assert_eq!(report.hits, 1);
        assert_eq!(
            output,
            [0.75f32, -0.375, 0.0]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn shared_local_seeded_chart_keeps_every_capture_and_record_preview_compatible() {
    use beatkernel::{
        input::{
            Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
            PhysicalControlId, PhysicalInputEvent,
        },
        judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
        time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
        transport::{Rate, Transport},
    };
    use std::collections::BTreeMap;
    struct Identity;
    impl ClockMapper for Identity {
        fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
            (from.domain == to).then_some(from.timestamp)
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Exact
        }
    }
    let point = ClockPoint {
        domain: ClockDomainId(17),
        timestamp: Timestamp::ZERO,
    };
    let dir = Directory::new();
    dir.write("head.wav", &wav(1, &[16384]));
    let path = dir.write("local.bms", b"#BPM 60\n#RANDOM 2\n#IF 1\n#WAV01 head.wav\n#00011:01\n#ELSE\n#WAV02 missing.wav\n#00012:02\n#ENDIF\n");
    let prepared = load_prepared_with_seed(
        &path,
        AudioFormat::new(24_000, 1).unwrap(),
        limits(),
        ChannelPolicy::Exact,
        3,
    )
    .unwrap();
    let control = prepared.source.notes[0].lane.control();
    let physical = PhysicalControlId::keyboard(7);
    let replay_limits = competition_live::replay_limits().unwrap();
    let draft = settings::NativeSettings::from_args(
        &["--chart-seed".into(), "3".into()],
        settings::SettingsHost::Linux,
    )
    .unwrap();
    for count in [2usize, 3, 4, 64] {
        let mut members = Vec::new();
        let mut captures = BTreeMap::new();
        for index in 0..count {
            let id = if index + 1 == count {
                u32::MAX
            } else {
                7 + index as u32
            };
            let player = local_players::PlayerId(id);
            let judge = JudgeEngine::new(
                prepared.compiled.chart.clone(),
                prepared.source.rules(),
                JudgeProfile::new(
                    vec![JudgeWindow {
                        grade: JudgeGrade(1),
                        early: Duration::from_nanos(150_000_000),
                        late: Duration::from_nanos(150_000_000),
                    }],
                    Duration::ZERO,
                )
                .unwrap(),
            )
            .unwrap();
            captures.insert(
                player,
                replay_capture::LiveReplayCapture::new_at_with_chart_seed(
                    &judge,
                    point.domain,
                    replay_limits,
                    Timestamp::ZERO,
                    3,
                )
                .unwrap(),
            );
            members.push(local_runtime::MemberConfig {
                player,
                device: Some(DeviceId(u64::from(id))),
                bindings: BindingMap::from_bindings([Binding {
                    device: DeviceSelector::Exact(DeviceId(u64::from(id))),
                    physical,
                    game_control: control,
                }])
                .unwrap(),
                judge,
                sounds: vec![],
            });
        }
        let players = members
            .iter()
            .map(|member| member.player)
            .collect::<Vec<_>>();
        let (producer, _consumer) = command_queue(1).unwrap();
        let mut group = local_runtime::RuntimeGroup::new(
            point.domain,
            point.domain,
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            producer,
            members,
            0,
            &[],
        )
        .unwrap();
        for player in players {
            let event = PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(DeviceId(u64::from(player.0)), point, 0),
                control: physical,
                state: ButtonState::Down,
            });
            let local_runtime::InputResult::Processed(reports) =
                group.process_input(event, &Identity, point).unwrap()
            else {
                panic!("assigned input must be processed")
            };
            assert_eq!(reports.len(), 1);
            assert_eq!(reports[0].player, player);
            assert_eq!(reports[0].report.judge_events.len(), 1);
            captures
                .get_mut(&player)
                .unwrap()
                .record_report(&reports[0].report)
                .unwrap();
        }
        let mut header = None;
        for (player, capture) in captures {
            let file = capture.into_file();
            assert_eq!(
                replay_playback::decode_chart_setup(&file.header.options)
                    .unwrap()
                    .2,
                3
            );
            if let Some(expected) = &header {
                assert_eq!(&file.header, expected);
            } else {
                header = Some(file.header.clone());
            }
            let replay =
                replay_playback::reconstruct(&prepared.source, file.clone(), replay_limits)
                    .unwrap();
            assert_eq!(replay.results().len(), 1);
            assert_eq!(
                replay.engine().stable_hash().unwrap(),
                group.member_judge(player).unwrap().stable_hash().unwrap()
            );
            let record = dir.write(
                &format!("local-{count}-{}.bkr", player.0),
                &beatkernel::replay::codec::encode_replay(&file, replay_limits).unwrap(),
            );
            let preview = record_catalog::RecordPreview::inspect(&record, &path, &draft).unwrap();
            assert_eq!(preview.score.hits, 1);
            let wrong =
                settings::NativeSettings::from_args(&[], settings::SettingsHost::Linux).unwrap();
            assert!(
                record_catalog::RecordPreview::inspect(&record, &path, &wrong)
                    .unwrap_err()
                    .contains("chart seed")
            );
            let mut competition = competition::Competition::new(file.header.clone(), 8).unwrap();
            competition
                .add_replay(
                    &prepared.source,
                    file.clone(),
                    replay_limits,
                    competition::OpponentKind::Own,
                    "same-seed",
                )
                .unwrap();
            let mut other_seed = file;
            let prefix = b"bms-judge-profile/v3:".len();
            other_seed.header.options[prefix..prefix + 8].copy_from_slice(&4u64.to_le_bytes());
            assert!(
                competition
                    .add_replay(
                        &prepared.source,
                        other_seed,
                        replay_limits,
                        competition::OpponentKind::Other,
                        "other-seed"
                    )
                    .is_err()
            );
        }
    }
}

#[test]
fn actual_hold_results_preserve_old_progress_and_match_replay_prefixes() {
    use beatkernel::{
        input::{
            Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
            PhysicalControlId, PhysicalInputEvent,
        },
        judge::{JudgeEngine, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeWindow, MissReason},
        runtime::Runtime,
        time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
        transport::{Rate, Transport},
    };
    use note_progress::{NoteProgress, NoteState};
    struct Identity;
    impl ClockMapper for Identity {
        fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
            (from.domain == to).then_some(from.timestamp)
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Exact
        }
    }
    let source = beatkernel_bms::parse(
        "#BPM 60\n#WAV01 hold.wav\n#00051:0101\n#00012:01\n",
        beatkernel_bms::ParseOptions::default(),
    )
    .unwrap();
    let compiled = source.compile().unwrap();
    let display = std::sync::Arc::new(
        player_chart::PlayerChart::from_compiled(&source, &compiled.chart).unwrap(),
    );
    let hold_index = display
        .notes
        .iter()
        .position(|note| note.end.is_some())
        .unwrap();
    let instant_index = display
        .notes
        .iter()
        .position(|note| note.end.is_none())
        .unwrap();
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::from_nanos(100_000_000),
            late: Duration::from_nanos(100_000_000),
        }],
        Duration::ZERO,
    )
    .unwrap();
    let judge = JudgeEngine::new(compiled.chart.clone(), source.rules(), profile).unwrap();
    let limits = competition_live::replay_limits().unwrap();
    let mut capture =
        replay_capture::LiveReplayCapture::new(&judge, ClockDomainId(17), limits).unwrap();
    let bindings = BindingMap::from_bindings([(4, 0x11), (5, 0x12)].into_iter().map(
        |(key, game)| Binding {
            device: DeviceSelector::Exact(DeviceId(3)),
            physical: PhysicalControlId::keyboard(key),
            game_control: beatkernel::input::GameControlId(game),
        },
    ))
    .unwrap();
    let (producer, _consumer) = command_queue(1).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(17),
        ClockDomainId(17),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    let point = |ns| ClockPoint {
        domain: ClockDomainId(17),
        timestamp: Timestamp::from_nanos(ns),
    };
    let mut live = NoteProgress::new(std::sync::Arc::clone(&display)).unwrap();
    let pending = live.clone();
    let mut reports = Vec::new();
    let mut prefixes = Vec::new();
    for (index, (key, state, ns)) in [
        (4, ButtonState::Down, 0),
        (5, ButtonState::Down, 0),
        (4, ButtonState::Up, 500_000_000),
    ]
    .into_iter()
    .enumerate()
    {
        let report = runtime
            .process_input(
                PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(3), point(ns), index as u64),
                    control: PhysicalControlId::keyboard(key),
                    state,
                }),
                &Identity,
                point(ns),
            )
            .unwrap();
        assert!(report.judge_error.is_none());
        assert_eq!(report.judge_events.len(), 1);
        if index == 2 {
            assert_eq!(report.judge_events[0].stage, JudgeStage::HoldTail);
            assert_eq!(
                report.judge_events[0].outcome,
                JudgeOutcome::Miss {
                    reason: MissReason::EarlyRelease
                }
            );
        }
        capture.record_report(&report).unwrap();
        live.apply(&report.judge_events);
        prefixes.push(live.clone());
        reports.push(report);
    }
    assert_eq!(pending.state(hold_index), Some(NoteState::Pending));
    assert_eq!(prefixes[0].state(hold_index), Some(NoteState::Holding));
    assert_eq!(prefixes[0].state(instant_index), Some(NoteState::Pending));
    assert_eq!(prefixes[1].state(instant_index), Some(NoteState::Completed));
    assert_eq!(live.state(hold_index), Some(NoteState::Completed));
    let file = capture.into_file();
    let encoded = beatkernel::replay::codec::encode_replay(&file, limits).unwrap();
    let file = beatkernel::replay::codec::decode_replay(&encoded, limits).unwrap();
    let mut visual = replay_visual::ReplayVisual::new(&source, &file, limits).unwrap();
    let mut replay = NoteProgress::new(std::sync::Arc::clone(&display)).unwrap();
    replay.apply(&visual.advance_to(Timestamp::ZERO).unwrap());
    assert_eq!(replay.state(hold_index), prefixes[1].state(hold_index));
    assert_eq!(
        replay.state(instant_index),
        prefixes[1].state(instant_index)
    );
    replay.apply(
        &visual
            .advance_to(Timestamp::from_nanos(500_000_000))
            .unwrap(),
    );
    assert_eq!(replay.state(hold_index), live.state(hold_index));
    assert_eq!(replay.state(instant_index), live.state(instant_index));
    let mut restored = replay_playback::reconstruct(&source, file.clone(), limits).unwrap();
    restored.seek_cursor(file.records.len()).unwrap();
    assert_eq!(
        restored.engine().stable_hash().unwrap(),
        runtime.judge().stable_hash().unwrap()
    );
    restored.seek_cursor(0).unwrap();
    let mut fresh = NoteProgress::new(std::sync::Arc::clone(&display)).unwrap();
    fresh.apply(restored.results());
    assert_eq!(fresh.state(hold_index), Some(NoteState::Pending));
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_chart(&source, &compiled.chart).map_err(|e| e.to_string())?;
        for report in &reports {
            player::publish_report(report).map_err(|e| e.to_string())?;
        }
        player::publish_pause(player::PauseState::Running);
        let shown = viewer.take_latest().unwrap();
        assert_eq!(
            shown.note_progress.as_ref().unwrap().state(hold_index),
            Some(NoteState::Completed)
        );
        player::publish_pause(player::PauseState::Paused);
        assert_eq!(
            viewer
                .take_latest()
                .unwrap()
                .note_progress
                .as_ref()
                .unwrap()
                .state(hold_index),
            Some(NoteState::Completed)
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(
        viewer
            .take_latest()
            .unwrap()
            .note_progress
            .as_ref()
            .unwrap()
            .state(hold_index),
        Some(NoteState::Completed)
    );
}

#[test]
fn actual_early_late_and_timeout_results_match_full_and_visual_replay_timing() {
    use beatkernel::{
        input::{
            Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
            GameControlId, PhysicalControlId, PhysicalInputEvent,
        },
        judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
        runtime::Runtime,
        time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
        transport::{Rate, Transport},
    };
    struct Identity;
    impl ClockMapper for Identity {
        fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
            (from.domain == to).then_some(from.timestamp)
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Exact
        }
    }
    let source = beatkernel_bms::parse(
        "#BPM 120\n#WAV01 tap.wav\n#00111:01\n#00112:01\n#00213:01\n",
        beatkernel_bms::ParseOptions::default(),
    )
    .unwrap();
    let compiled = source.compile().unwrap();
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(7),
            early: Duration::from_nanos(100_000_000),
            late: Duration::from_nanos(100_000_000),
        }],
        Duration::ZERO,
    )
    .unwrap();
    let judge = JudgeEngine::new(compiled.chart.clone(), source.rules(), profile).unwrap();
    let limits = competition_live::replay_limits().unwrap();
    let mut capture =
        replay_capture::LiveReplayCapture::new(&judge, ClockDomainId(17), limits).unwrap();
    let bindings = BindingMap::from_bindings([(4, 0x11), (5, 0x12)].into_iter().map(
        |(key, game)| Binding {
            device: DeviceSelector::Exact(DeviceId(3)),
            physical: PhysicalControlId::keyboard(key),
            game_control: GameControlId(game),
        },
    ))
    .unwrap();
    let (producer, _consumer) = command_queue(1).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(17),
        ClockDomainId(17),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    let point = |ns| ClockPoint {
        domain: ClockDomainId(17),
        timestamp: Timestamp::from_nanos(ns),
    };
    let mut reports = Vec::new();
    for (sequence, (key, ns)) in [(4, 1_990_000_000), (5, 2_020_000_000)]
        .into_iter()
        .enumerate()
    {
        reports.push(
            runtime
                .process_input(
                    PhysicalInputEvent::Button(ButtonEvent {
                        meta: EventMeta::new(DeviceId(3), point(ns), sequence as u64),
                        control: PhysicalControlId::keyboard(key),
                        state: ButtonState::Down,
                    }),
                    &Identity,
                    point(ns),
                )
                .unwrap(),
        );
    }
    reports.push(
        runtime
            .advance_to(point(4_200_000_000), &Identity, point(4_200_000_000))
            .unwrap(),
    );
    let mut live = competition::ScoreSummary::default();
    for report in &reports {
        assert!(report.judge_error.is_none());
        assert_eq!(report.judge_events.len(), 1);
        live.observe(&report.judge_events).unwrap();
        capture.record_report(report).unwrap();
    }
    assert_eq!((live.hits, live.misses), (2, 1));
    assert_eq!(
        (
            live.timing.count(),
            live.timing.early(),
            live.timing.late(),
            live.timing.exact()
        ),
        (2, 1, 1, 0)
    );
    assert_eq!(live.timing.mean_ns(), Some(5_000_000));
    assert_eq!(live.timing.mean_absolute_ns(), Some(15_000_000));
    assert_eq!(
        timing_display::summary(&live.timing),
        ("BIAS +5.000 MS".into(), "MEAN ABS 15.000 MS".into())
    );
    let file = capture.into_file();
    let bytes = beatkernel::replay::codec::encode_replay(&file, limits).unwrap();
    let file = beatkernel::replay::codec::decode_replay(&bytes, limits).unwrap();
    let mut restored = replay_playback::reconstruct(&source, file.clone(), limits).unwrap();
    restored.seek_cursor(file.records.len()).unwrap();
    let mut summary = competition::ScoreSummary::default();
    summary.observe(restored.results()).unwrap();
    assert_eq!(summary, live);
    assert_eq!(
        restored.engine().stable_hash().unwrap(),
        runtime.judge().stable_hash().unwrap()
    );
    let mut visual = replay_visual::ReplayVisual::new(&source, &file, limits).unwrap();
    let mut prefix = competition::ScoreSummary::default();
    prefix
        .observe(
            &visual
                .advance_to(Timestamp::from_nanos(1_990_000_000))
                .unwrap(),
        )
        .unwrap();
    assert_eq!((prefix.timing.count(), prefix.timing.early()), (1, 1));
    assert_eq!(prefix.timing.mean_ns(), Some(-10_000_000));
    prefix
        .observe(
            &visual
                .advance_to(Timestamp::from_nanos(2_020_000_000))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(prefix.timing, live.timing);
    prefix
        .observe(
            &visual
                .advance_to(Timestamp::from_nanos(4_200_000_000))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(prefix, live); // Timeout changes counts, never fabricates an error sample.
    restored.seek_cursor(0).unwrap();
    let mut empty = competition::ScoreSummary::default();
    empty.observe(restored.results()).unwrap();
    assert_eq!(empty.timing.count(), 0);
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_chart(&source, &compiled.chart).map_err(|e| e.to_string())?;
        for report in &reports {
            player::publish_report(report).map_err(|e| e.to_string())?;
        }
        player::publish_pause(player::PauseState::Running);
        let shown = viewer.take_latest().unwrap();
        assert_eq!(shown.score.timing, live.timing);
        assert_eq!(shown.players[0].score.timing, live.timing);
        Ok(())
    })
    .unwrap();
    assert_eq!(viewer.take_latest().unwrap().score.timing, live.timing);
}

#[test]
fn actual_runtime_replay_and_fresh_practice_share_pre_stop_bga_song_time() {
    use beatkernel::{
        input::{
            Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
            GameControlId, PhysicalControlId, PhysicalInputEvent,
        },
        judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
        runtime::Runtime,
        time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
        transport::{Rate, Transport},
    };
    use beatkernel_bms::ImageId;
    struct Identity;
    impl ClockMapper for Identity {
        fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
            (from.domain == to).then_some(from.timestamp)
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Exact
        }
    }
    let text = "#BPM 120\n#WAV01 tap.wav\n#00111:01\n#STOP01 48\n#00109:01\n\
         #BMP00 poor.png\n#BMP01 first.png\n#BMP02 second.png\n#BMP03 third.png\n\
         #BMP04 layer.png\n#00004:01\n#00104:0203\n#00107:04\n#00106:ZZ\n";
    let source = beatkernel_bms::parse(text, beatkernel_bms::ParseOptions::default()).unwrap();
    let image_dir = Directory::new();
    let chart_path = image_dir.write("chart.bms", text.as_bytes());
    for (index, name) in ["first.png", "second.png", "third.png", "layer.png"]
        .into_iter()
        .enumerate()
    {
        image_dir.write(name, &raster_bmp_pixel([index as u8 + 1, 0, 0]));
    }
    let compiled = source.compile().unwrap();
    let chart = player_chart::PlayerChart::from_compiled(&source, &compiled.chart).unwrap();
    let expected = bga::BgaState {
        base: Some(ImageId(2)),
        layer: Some(ImageId(4)),
        poor: Some(ImageId(1295)),
    };
    assert_eq!(
        chart.bga_state(Timestamp::from_nanos(2_000_000_000)),
        expected
    );
    assert_eq!(
        chart.bga_state(Timestamp::from_nanos(3_499_999_999)),
        expected
    );
    assert_eq!(
        chart.bga_state(Timestamp::from_nanos(3_500_000_000)).base,
        Some(ImageId(3))
    );
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::from_nanos(100_000_000),
            late: Duration::from_nanos(100_000_000),
        }],
        Duration::ZERO,
    )
    .unwrap();
    let judge = JudgeEngine::new(compiled.chart.clone(), source.rules(), profile.clone()).unwrap();
    let limits = competition_live::replay_limits().unwrap();
    let mut capture =
        replay_capture::LiveReplayCapture::new(&judge, ClockDomainId(17), limits).unwrap();
    let binding = || {
        BindingMap::from_bindings([Binding {
            device: DeviceSelector::Exact(DeviceId(3)),
            physical: PhysicalControlId::keyboard(4),
            game_control: GameControlId(0x11),
        }])
        .unwrap()
    };
    let (producer, _consumer) = command_queue(1).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(17),
        ClockDomainId(17),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        binding(),
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    let point = |ns| ClockPoint {
        domain: ClockDomainId(17),
        timestamp: Timestamp::from_nanos(ns),
    };
    let hit = runtime
        .process_input(
            PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(DeviceId(3), point(2_000_000_000), 0),
                control: PhysicalControlId::keyboard(4),
                state: ButtonState::Down,
            }),
            &Identity,
            point(2_000_000_000),
        )
        .unwrap();
    assert!(hit.judge_error.is_none());
    assert_eq!(hit.judge_events.len(), 1);
    capture.record_report(&hit).unwrap();
    let during_stop = runtime
        .advance_to(point(2_250_000_000), &Identity, point(2_250_000_000))
        .unwrap();
    capture.record_report(&during_stop).unwrap();
    let later = runtime
        .advance_to(point(3_500_000_000), &Identity, point(3_500_000_000))
        .unwrap();
    capture.record_report(&later).unwrap();
    let file = capture.into_file();
    let file = beatkernel::replay::codec::decode_replay(
        &beatkernel::replay::codec::encode_replay(&file, limits).unwrap(),
        limits,
    )
    .unwrap();
    let mut restored = replay_playback::reconstruct(&source, file.clone(), limits).unwrap();
    restored.seek_cursor(file.records.len()).unwrap();
    assert_eq!(
        restored.engine().stable_hash().unwrap(),
        runtime.judge().stable_hash().unwrap()
    );
    let mut replay = replay_visual::ReplayVisual::new(&source, &file, limits).unwrap();
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_native_chart(
            &chart_path,
            &source,
            &compiled.chart,
            &[local_players::PlayerId(1)],
        )
        .map_err(|e| e.to_string())?;
        for report in [&hit, &during_stop, &later] {
            player::publish_report(report).map_err(|e| e.to_string())?;
            // Force the coalescing bridge without sleeps; this unit acknowledgment
            // does not claim real native-device pause behavior.
            player::publish_pause(player::PauseState::Running);
            let shown = viewer.take_latest().unwrap();
            let now = shown.song_time.unwrap();
            let live_state = shown.chart.as_ref().unwrap().bga_state(now);
            let base = live_state.base.unwrap();
            assert_eq!(
                shown.images.as_ref().unwrap().get(base).unwrap().pixels(),
                &[base.0 as u8, 0, 0, 255]
            );
            assert_eq!(
                shown.players[0].chart.as_ref().unwrap().bga_state(now),
                live_state
            );
            let events = replay.advance_to(now).unwrap();
            player::publish_replay_prefix(now, &events).map_err(|e| e.to_string())?;
            player::publish_pause(player::PauseState::Paused);
            let replay_shown = viewer.take_latest().unwrap();
            assert!(std::sync::Arc::ptr_eq(
                shown.images.as_ref().unwrap(),
                replay_shown.images.as_ref().unwrap()
            ));
            assert_eq!(
                replay_shown
                    .chart
                    .as_ref()
                    .unwrap()
                    .bga_state(replay_shown.song_time.unwrap()),
                live_state
            );
        }
        player::publish_pause(player::PauseState::Running);
        player::publish_pause(player::PauseState::Paused);
        let paused = viewer.take_latest().unwrap();
        assert_eq!(
            paused
                .chart
                .as_ref()
                .unwrap()
                .bga_state(paused.song_time.unwrap()),
            chart.bga_state(later.song_time)
        );
        Ok(())
    })
    .unwrap();

    // A fresh native-style transport starts at the original song position;
    // image state does not replay elapsed host time or reuse a forward cursor.
    let (producer, _fresh_consumer) = command_queue(1).unwrap();
    let mut fresh = Runtime::new(
        ClockDomainId(17),
        ClockDomainId(17),
        Transport::new(
            Timestamp::ZERO,
            Timestamp::from_nanos(2_250_000_000),
            Rate::NORMAL,
        ),
        binding(),
        JudgeEngine::new(compiled.chart.clone(), source.rules(), profile).unwrap(),
        producer,
        vec![],
        0,
    )
    .unwrap();
    let restarted = fresh.advance_to(point(0), &Identity, point(0)).unwrap();
    assert_eq!(restarted.song_time, during_stop.song_time);
    assert_eq!(chart.bga_state(restarted.song_time), expected);
    assert_eq!(chart.bga_state(Timestamp::ZERO).base, Some(ImageId(1)));
}

fn raster_bmp_pixel(rgb: [u8; 3]) -> Vec<u8> {
    // Original one-pixel 24-bit BMP with its four-byte padded row.
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

#[test]
fn native_layer_preparation_preserves_raw_aliases_and_admits_variant_bytes_atomically() {
    use beatkernel_bms::ImageId;
    use image_assets::{ImageAssetLimits, ImageAssets};
    use std::sync::Arc;
    let dir = Directory::new();
    dir.write("tap.wav", &wav(1, &[100, -100]));
    dir.write("black.bmp", &raster_bmp_pixel([0, 0, 0]));
    let path = dir.write("chart.bms", b"#BPM 120\n#WAV01 tap.wav\n#00011:01\n#BMP01 black.bmp\n#BMP02 ./black.bmp\n#BMP03 black.bmp\n#00004:010203\n#00007:0102\n");
    let prepared = load_prepared(
        &path,
        AudioFormat::new(24_000, 2).unwrap(),
        limits(),
        ChannelPolicy::MonoToStereo,
    )
    .unwrap();
    let identity = captured_setup_identity(&prepared);
    let bank = ImageAssets::prepare(
        &dir.0,
        &prepared.source,
        ImageAssetLimits {
            max_decoded_bytes: 8,
            ..ImageAssetLimits::default()
        },
    )
    .unwrap();
    assert_eq!((bank.unique_images(), bank.decoded_bytes()), (1, 8));
    assert!(Arc::ptr_eq(
        bank.get(ImageId(1)).unwrap(),
        bank.get(ImageId(3)).unwrap()
    ));
    assert!(Arc::ptr_eq(
        bank.get_layer(ImageId(1)).unwrap(),
        bank.get_layer(ImageId(2)).unwrap()
    ));
    assert!(!Arc::ptr_eq(
        bank.get(ImageId(1)).unwrap(),
        bank.get_layer(ImageId(1)).unwrap()
    ));
    assert_eq!(bank.get(ImageId(1)).unwrap().pixels(), &[0, 0, 0, 255]);
    assert_eq!(bank.get_layer(ImageId(1)).unwrap().pixels(), &[0, 0, 0, 0]);
    assert!(bank.get_layer(ImageId(3)).is_none());
    assert!(
        ImageAssets::prepare(
            &dir.0,
            &prepared.source,
            ImageAssetLimits {
                max_decoded_bytes: 7,
                ..ImageAssetLimits::default()
            }
        )
        .is_err()
    );
    assert_eq!(bank.get(ImageId(1)).unwrap().pixels(), &[0, 0, 0, 255]);
    assert_eq!(identity, captured_setup_identity(&prepared));
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_native_chart(
            &path,
            &prepared.source,
            &prepared.compiled.chart,
            &[local_players::PlayerId(1)],
        )
        .map_err(|e| e.to_string())?;
        let published = viewer.take_latest().unwrap();
        let images = published.images.as_ref().unwrap();
        assert_eq!(images.decoded_bytes(), 8);
        assert_eq!(images.get(ImageId(2)).unwrap().pixels(), &[0, 0, 0, 255]);
        assert_eq!(
            images.get_layer(ImageId(2)).unwrap().pixels(),
            &[0, 0, 0, 0]
        );
        assert!(Arc::ptr_eq(
            images.get_layer(ImageId(1)).unwrap(),
            images.get_layer(ImageId(2)).unwrap()
        ));
        Ok(())
    })
    .unwrap();
}

#[test]
fn prepared_audio_and_image_bank_share_real_chart_selection_without_frame_io() {
    use beatkernel::{audio::AudioFormat, time::Timestamp};
    use beatkernel_bms::ImageId;
    use image_assets::{ImageAssetLimits, ImageAssets, ImageUnavailable};
    let dir = Directory::new();
    dir.write("tap.wav", &wav(1, &[100, -100]));
    dir.write("背景.BMP", &raster_bmp_pixel([255, 64, 0]));
    dir.write("video.mpg", b"unsupported video bytes");
    dir.write("broken.png", b"\x89PNG\r\n\x1a\nbad");
    let path = dir.write(
        "chart.bms",
        b"#BPM 120\n#WAV01 tap.wav\n#00011:01\n\
        #BMP00 missing-poor.png\n#BMP01 \xe8\x83\x8c\xe6\x99\xaf.BMP\n\
        #BMP02 ./\xe8\x83\x8c\xe6\x99\xaf.BMP\n#BMP03 video.mpg\n#BMP04 broken.png\n\
        #BMP05 ../unused-escape.bmp\n#BMP06 missing.jpg\n#00004:0102030406ZZ\n",
    );
    let prepared = load_prepared(
        &path,
        AudioFormat::new(24_000, 2).unwrap(),
        limits(),
        ChannelPolicy::MonoToStereo,
    )
    .unwrap();
    let bank = ImageAssets::prepare(
        &dir.0,
        &prepared.source,
        ImageAssetLimits {
            max_decoded_bytes: 4,
            ..ImageAssetLimits::default()
        },
    )
    .unwrap(); // Canonical aliases consume the exact four-byte budget once.
    assert_eq!(
        (bank.len(), bank.unique_images(), bank.decoded_bytes()),
        (7, 1, 4)
    );
    assert!(std::sync::Arc::ptr_eq(
        bank.get(ImageId(1)).unwrap(),
        bank.get(ImageId(2)).unwrap()
    ));
    assert_eq!(bank.get(ImageId(1)).unwrap().pixels(), &[255, 64, 0, 255]);
    assert_eq!(
        bank.unavailable(ImageId(0)),
        Some(&ImageUnavailable::Missing)
    );
    assert_eq!(
        bank.unavailable(ImageId(3)),
        Some(&ImageUnavailable::Unsupported)
    );
    assert!(matches!(
        bank.unavailable(ImageId(4)),
        Some(ImageUnavailable::InvalidData(_))
    ));
    assert_eq!(
        bank.unavailable(ImageId(6)),
        Some(&ImageUnavailable::Missing)
    );
    assert_eq!(
        bank.unavailable(ImageId(1295)),
        Some(&ImageUnavailable::Undefined)
    );
    assert_eq!(bank.unavailable(ImageId(5)), None); // Unused unsafe definition never opened.
    let chart =
        player_chart::PlayerChart::from_compiled(&prepared.source, &prepared.compiled.chart)
            .unwrap();
    let selected = chart.bga_state(Timestamp::ZERO).base.unwrap();
    assert_eq!(selected, ImageId(1));
    assert_eq!(bank.get(selected).unwrap().width(), 1);
    // Removing the source file after preparation cannot affect stored selections or pixels.
    std::fs::remove_file(dir.0.join("背景.BMP")).unwrap();
    assert_eq!(bank.get(selected).unwrap().pixels(), &[255, 64, 0, 255]);
    assert_eq!(chart.bga_state(Timestamp::ZERO).base, Some(selected));
}

#[test]
fn image_preparation_caps_and_unsafe_paths_reject_without_partial_bank() {
    use beatkernel_bms::ImageId;
    use image_assets::{ImageAssetLimits, ImageAssets};
    let dir = Directory::new();
    dir.write("first.bmp", &raster_bmp_pixel([1, 2, 3]));
    dir.write("second.bmp", &raster_bmp_pixel([4, 5, 6]));
    let source = beatkernel_bms::parse(
        "#BMP01 first.bmp\n#BMP02 second.bmp\n#00004:0102",
        beatkernel_bms::ParseOptions::default(),
    )
    .unwrap();
    let accepted = ImageAssets::prepare(&dir.0, &source, ImageAssetLimits::default()).unwrap();
    assert_eq!(accepted.decoded_bytes(), 8);
    let cap = ImageAssetLimits {
        max_images: 1,
        ..ImageAssetLimits::default()
    };
    assert!(
        ImageAssets::prepare(&dir.0, &source, cap)
            .err()
            .unwrap()
            .contains("capacity")
    );
    let cap = ImageAssetLimits {
        max_decoded_bytes: 7,
        ..ImageAssetLimits::default()
    };
    assert!(
        ImageAssets::prepare(&dir.0, &source, cap)
            .err()
            .unwrap()
            .contains("budget")
    );
    let cap = ImageAssetLimits {
        decode: image_decode::ImageDecodeLimits {
            max_encoded_bytes: 57,
            ..image_decode::ImageDecodeLimits::default()
        },
        ..ImageAssetLimits::default()
    };
    assert!(
        ImageAssets::prepare(&dir.0, &source, cap)
            .err()
            .unwrap()
            .contains("limit")
    );
    assert_eq!(accepted.get(ImageId(2)).unwrap().pixels(), &[4, 5, 6, 255]);
    let mut escaped = source.clone();
    escaped.images.insert(ImageId(1), "../outside.bmp".into());
    assert!(ImageAssets::prepare(&dir.0, &escaped, ImageAssetLimits::default()).is_err());
    std::fs::create_dir(dir.0.join("directory.bmp")).unwrap();
    escaped.images.insert(ImageId(1), "directory.bmp".into());
    assert!(ImageAssets::prepare(&dir.0, &escaped, ImageAssetLimits::default()).is_err());
    #[cfg(unix)]
    {
        let outside = Directory::new();
        outside.write("outside.bmp", &raster_bmp_pixel([7, 8, 9]));
        std::os::unix::fs::symlink(outside.0.join("outside.bmp"), dir.0.join("escape.bmp"))
            .unwrap();
        escaped.images.insert(ImageId(1), "escape.bmp".into());
        assert!(ImageAssets::prepare(&dir.0, &escaped, ImageAssetLimits::default()).is_err());
    }
    for cap in [
        ImageAssetLimits {
            max_images: 0,
            ..ImageAssetLimits::default()
        },
        ImageAssetLimits {
            max_decoded_bytes: image_assets::MAX_IMAGE_BANK_BYTES + 1,
            ..ImageAssetLimits::default()
        },
    ] {
        assert!(ImageAssets::prepare(&dir.0, &source, cap).is_err());
    }
}

#[test]
fn native_chart_publication_is_atomic_shares_assets_and_resets_fresh_sessions() {
    use beatkernel_bms::ImageId;
    use local_players::PlayerId;
    let dir = Directory::new();
    dir.write("tap.wav", &wav(1, &[100, -100]));
    dir.write("image.bmp", &raster_bmp_pixel([240, 20, 80]));
    let path = dir.write("chart.bms", b"#BPM 120\n#WAV01 tap.wav\n#00011:01\n#BMP01 image.bmp\n#BMP02 ./image.bmp\n#00004:01\n#00007:02\n");
    let prepared = load_prepared(
        &path,
        AudioFormat::new(24_000, 2).unwrap(),
        limits(),
        ChannelPolicy::MonoToStereo,
    )
    .unwrap();
    let absent = dir.0.join("absent-chart.bms");
    // Unattached native audio-only execution retains the legacy no-image-IO path.
    player::publish_native_chart(
        &absent,
        &prepared.source,
        &prepared.compiled.chart,
        &[PlayerId(1)],
    )
    .unwrap();
    assert!(
        player::publish_native_chart(
            &absent,
            &prepared.source,
            &prepared.compiled.chart,
            &[PlayerId(0)]
        )
        .is_err()
    );
    let (publisher, viewer) = player::channel();
    let retained = player::with_publisher(publisher, || {
        let mut unsafe_source = prepared.source.clone();
        unsafe_source
            .images
            .insert(ImageId(1), "../escape.bmp".into());
        assert!(
            player::publish_native_chart(
                &path,
                &unsafe_source,
                &prepared.compiled.chart,
                &[PlayerId(7), PlayerId(u32::MAX)]
            )
            .is_err()
        );
        let rejected = viewer.take_latest().unwrap();
        assert!(
            rejected.chart.is_none() && rejected.images.is_none() && rejected.players.is_empty()
        );
        player::publish_native_chart(
            &path,
            &prepared.source,
            &prepared.compiled.chart,
            &[PlayerId(7), PlayerId(u32::MAX)],
        )
        .map_err(|e| e.to_string())?;
        let shown = viewer.take_latest().unwrap();
        assert_eq!(
            shown.players.iter().map(|p| p.player).collect::<Vec<_>>(),
            [PlayerId(7), PlayerId(u32::MAX)]
        );
        assert!(std::sync::Arc::ptr_eq(
            shown.players[0].chart.as_ref().unwrap(),
            shown.players[1].chart.as_ref().unwrap()
        ));
        let images = shown.images.clone().unwrap();
        assert_eq!((images.unique_images(), images.decoded_bytes()), (1, 4));
        assert!(std::sync::Arc::ptr_eq(
            images.get(ImageId(1)).unwrap(),
            images.get(ImageId(2)).unwrap()
        ));
        assert_eq!(
            images.get(ImageId(1)).unwrap().pixels(),
            &[240, 20, 80, 255]
        );
        // Re-registration rejects before trying to open the absent chart.
        assert!(
            player::publish_native_chart(
                &absent,
                &prepared.source,
                &prepared.compiled.chart,
                &[PlayerId(7), PlayerId(u32::MAX)]
            )
            .err()
            .unwrap()
            .to_string()
            .contains("already registered")
        );
        player::publish_pause(player::PauseState::Running);
        let running = viewer.take_latest().unwrap();
        player::publish_pause(player::PauseState::Paused);
        let paused = viewer.take_latest().unwrap();
        assert!(std::sync::Arc::ptr_eq(
            running.images.as_ref().unwrap(),
            paused.images.as_ref().unwrap()
        ));
        viewer.cancel();
        Ok(images)
    })
    .unwrap();
    let terminal = viewer.take_latest().unwrap();
    assert!(terminal.cancelled);
    assert!(std::sync::Arc::ptr_eq(
        &retained,
        terminal.images.as_ref().unwrap()
    ));
    let (publisher, fresh_viewer) = player::channel();
    player::with_publisher(publisher, || {
        let fresh = fresh_viewer.take_latest().unwrap();
        assert!(fresh.chart.is_none() && fresh.images.is_none());
        player::publish_chart(&prepared.source, &prepared.compiled.chart)
            .map_err(|e| e.to_string())?;
        assert!(fresh_viewer.take_latest().unwrap().images.is_none());
        Ok(())
    })
    .unwrap();
    assert_eq!(
        retained.get(ImageId(1)).unwrap().pixels(),
        &[240, 20, 80, 255]
    );
}
