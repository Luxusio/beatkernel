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
