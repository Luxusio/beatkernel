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
