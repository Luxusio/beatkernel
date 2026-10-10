//! Finite, authored malformed-media corpus through public decoding and preparation.
use beatkernel::audio::{AudioError, AudioFormat, PcmLimits, SampleId};
use beatkernel_bms_runtime::{
    AssetDecoder, ChannelPolicy, DefaultAssetDecoder, PreparedBms,
    asset_paths::AssetPathPolicy,
    asset_source::{MemoryAssetLimits, MemoryFiles},
    load_prepared, prepare_from_source,
};
use std::{
    error::Error,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[path = "../src/flac_fixture.rs"]
mod flac_fixture;
#[path = "../src/mp3_fixture.rs"]
#[allow(dead_code)]
mod mp3_fixture;
#[path = "../src/vorbis_fixture.rs"]
#[allow(dead_code)]
mod vorbis_fixture;

fn limits(asset: usize, total: usize, count: usize) -> PcmLimits {
    PcmLimits::new(asset, total, count).unwrap()
}

fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
    assert_eq!(samples.len() % usize::from(channels), 0);
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(36 + (samples.len() * 2) as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&((samples.len() * 2) as u32).to_le_bytes());
    for value in samples {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn mono_cases() -> Vec<(&'static str, Vec<u8>, u32, usize, Vec<f32>)> {
    vec![
        (
            "WAV",
            wav(22_050, 1, &[16_384, -16_384]),
            22_050,
            2,
            vec![0.5, -0.5],
        ),
        (
            "FLAC",
            flac_fixture::flac16(44_100, 1, &[16_384, -16_384], Some(2)),
            44_100,
            2,
            vec![0.5, -0.5],
        ),
        (
            "Vorbis",
            vorbis_fixture::silence(1, 48),
            24_000,
            48,
            vec![0.0; 48],
        ),
        (
            "MP3",
            mp3_fixture::silence(1, 1),
            24_000,
            576,
            vec![0.0; 576],
        ),
    ]
}

fn decode(bytes: &[u8], cap: usize) -> Result<beatkernel::audio::PcmSample, Box<dyn Error>> {
    DefaultAssetDecoder.decode(Path::new("misleading.wav"), bytes, limits(cap, cap, 8))
}

fn memory(chart: &[u8], assets: &[(&str, &[u8])]) -> MemoryFiles {
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files.insert("pack/chart.bms", chart.to_vec()).unwrap();
    for &(name, bytes) in assets {
        files
            .insert(&format!("pack/{name}"), bytes.to_vec())
            .unwrap();
    }
    files
}

fn prepare(
    files: &MemoryFiles,
    chart: &[u8],
    format: AudioFormat,
    cap: PcmLimits,
    policy: ChannelPolicy,
) -> Result<PreparedBms, Box<dyn Error>> {
    let source = files.scope("pack/chart.bms").unwrap();
    prepare_from_source(
        chart,
        &source,
        format,
        cap,
        policy,
        &DefaultAssetDecoder,
        AssetPathPolicy::Exact,
        0,
        None,
    )
}

fn one_chart(name: &str) -> Vec<u8> {
    format!("#BPM 120\n#WAV01 {name}\n#00011:01\n").into_bytes()
}

#[test]
fn four_codecs_decode_original_source_rates_and_prepare_exact_pcm() {
    for (codec, encoded, rate, frames, expected) in mono_cases() {
        let bytes = frames * 4;
        let pcm = decode(&encoded, bytes).unwrap_or_else(|error| panic!("{codec}: {error}"));
        assert_eq!(pcm.format(), AudioFormat::new(rate, 1).unwrap(), "{codec}");
        assert_eq!(pcm.frames(), frames, "{codec}");
        assert_eq!(pcm.samples(), expected, "{codec}");
        let chart = one_chart("tone.bin");
        let files = memory(&chart, &[("tone.bin", &encoded)]);
        let prepared = prepare(
            &files,
            &chart,
            AudioFormat::new(48_000, 1).unwrap(),
            limits(bytes, bytes, 1),
            ChannelPolicy::Exact,
        )
        .unwrap();
        assert_eq!(prepared.bank.len(), 1, "{codec}");
        assert_eq!(prepared.bank.total_bytes(), bytes, "{codec}");
        let sample = prepared.bank.get(SampleId(1)).unwrap();
        assert_eq!(
            sample.format(),
            AudioFormat::new(rate, 1).unwrap(),
            "{codec}"
        );
        assert_eq!(sample.samples(), expected, "{codec}");
    }
}

#[test]
fn distinct_source_rates_coexist_without_resampling_or_source_rate_matching() {
    let chart = b"#BPM 120\n#WAV01 low.wav\n#WAV02 high.flac\n#WAV03 mid.ogg\n#WAV04 mid.mp3\n#00011:01020304\n";
    let low = wav(22_050, 1, &[16_384]);
    let high = flac_fixture::flac16(44_100, 1, &[-16_384], Some(1));
    let mid_ogg = vorbis_fixture::silence(1, 1);
    let mid_mp3 = mp3_fixture::silence(1, 1);
    let files = memory(
        chart,
        &[
            ("low.wav", &low),
            ("high.flac", &high),
            ("mid.ogg", &mid_ogg),
            ("mid.mp3", &mid_mp3),
        ],
    );
    let prepared = prepare(
        &files,
        chart,
        AudioFormat::new(48_000, 1).unwrap(),
        limits(2304, 2316, 4),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert_eq!(prepared.bank.total_bytes(), 2316);
    for (id, rate, samples) in [
        (1, 22_050, 1),
        (2, 44_100, 1),
        (3, 24_000, 1),
        (4, 24_000, 576),
    ] {
        let pcm = prepared.bank.get(SampleId(id)).unwrap();
        assert_eq!(pcm.format(), AudioFormat::new(rate, 1).unwrap());
        assert_eq!(pcm.samples().len(), samples);
    }
}

#[test]
fn exact_and_one_byte_under_decoded_caps_are_distinct_for_every_codec() {
    for (codec, encoded, _, frames, _) in mono_cases() {
        let exact = frames * 4;
        assert!(decode(&encoded, exact).is_ok(), "{codec} exact");
        let error = decode(&encoded, exact - 1).unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains("capacity") || text.contains("limit") || text.contains("exceeds"),
            "{codec}: {text}"
        );
        let chart = one_chart("sample.bin");
        let files = memory(&chart, &[("sample.bin", &encoded)]);
        assert!(
            prepare(
                &files,
                &chart,
                AudioFormat::new(48_000, 1).unwrap(),
                limits(exact, exact, 1),
                ChannelPolicy::Exact
            )
            .is_ok(),
            "{codec}"
        );
        assert!(
            prepare(
                &files,
                &chart,
                AudioFormat::new(48_000, 1).unwrap(),
                limits(exact - 1, exact, 1),
                ChannelPolicy::Exact
            )
            .is_err(),
            "{codec}"
        );
    }
}

#[test]
fn mono_expansion_charges_asset_and_aggregate_after_channel_policy() {
    let chart = b"#BPM 120\n#WAV01 one.wav\n#WAV02 two.flac\n#00011:0102\n";
    let one = wav(22_050, 1, &[16_384, -16_384]);
    let two = flac_fixture::flac16(44_100, 1, &[8192, -8192], Some(2));
    let files = memory(chart, &[("one.wav", &one), ("two.flac", &two)]);
    let stereo = AudioFormat::new(48_000, 2).unwrap();
    let accepted = prepare(
        &files,
        chart,
        stereo,
        limits(16, 32, 2),
        ChannelPolicy::MonoToStereo,
    )
    .unwrap();
    assert_eq!((accepted.bank.len(), accepted.bank.total_bytes()), (2, 32));
    assert_eq!(
        accepted.bank.get(SampleId(1)).unwrap().samples(),
        &[0.5, 0.5, -0.5, -0.5]
    );
    assert_eq!(
        accepted.bank.get(SampleId(2)).unwrap().format(),
        AudioFormat::new(44_100, 2).unwrap()
    );
    assert!(
        prepare(
            &files,
            chart,
            stereo,
            limits(15, 32, 2),
            ChannelPolicy::MonoToStereo
        )
        .unwrap_err()
        .to_string()
        .contains("stereo expansion")
    );
    let total = prepare(
        &files,
        chart,
        stereo,
        limits(16, 31, 2),
        ChannelPolicy::MonoToStereo,
    )
    .unwrap_err();
    assert_eq!(
        total.downcast_ref::<AudioError>(),
        Some(&AudioError::PcmCapacity)
    );
    assert!(
        prepare(
            &files,
            chart,
            stereo,
            limits(16, 32, 1),
            ChannelPolicy::MonoToStereo
        )
        .is_err()
    );
    assert!(
        prepare(
            &files,
            chart,
            stereo,
            limits(16, 32, 2),
            ChannelPolicy::Exact
        )
        .is_err()
    );
}

#[test]
fn aliases_retain_distinct_ids_and_charge_each_owned_pcm_copy() {
    let chart = b"#BPM 120\n#WAV01 tap.wav\n#WAV02 ./tap.wav\n#00011:0102\n";
    let encoded = wav(24_000, 1, &[0, 16_384, -16_384]);
    let files = memory(chart, &[("tap.wav", &encoded)]);
    let format = AudioFormat::new(48_000, 2).unwrap();
    let accepted = prepare(
        &files,
        chart,
        format,
        limits(24, 48, 2),
        ChannelPolicy::MonoToStereo,
    )
    .unwrap();
    let first = accepted.bank.get(SampleId(1)).unwrap();
    let second = accepted.bank.get(SampleId(2)).unwrap();
    assert_eq!(accepted.bank.total_bytes(), 48);
    assert_eq!(first.samples(), second.samples());
    assert_ne!(first.samples().as_ptr(), second.samples().as_ptr());
    let error = prepare(
        &files,
        chart,
        format,
        limits(24, 47, 2),
        ChannelPolicy::MonoToStereo,
    )
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<AudioError>(),
        Some(&AudioError::PcmCapacity)
    );
    assert!(
        prepare(
            &files,
            chart,
            format,
            limits(24, 48, 1),
            ChannelPolicy::MonoToStereo
        )
        .is_err()
    );
}

#[test]
fn within_asset_format_changes_and_mixed_vorbis_chains_reject() {
    let mut flac = flac_fixture::flac16(48_000, 1, &[1, 2], Some(2));
    flac[45] = 0x18; // The frame says stereo although STREAMINFO says mono.
    flac_fixture::refresh_flac16_checksums(&mut flac);
    assert!(
        decode(&flac, 8)
            .unwrap_err()
            .to_string()
            .contains("channels or bit depth")
    );
    let mut mp3 = mp3_fixture::silence(1, 2);
    mp3[192 + 3] = 0; // Second frame changes from mono to stereo.
    assert!(
        decode(&mp3, 8192)
            .unwrap_err()
            .to_string()
            .contains("source format changed")
    );
    let mixed = [
        vorbis_fixture::link(vorbis_fixture::silence(1, 1), 101, 24_000),
        vorbis_fixture::link(vorbis_fixture::silence(1, 1), 202, 48_000),
    ]
    .concat();
    assert!(
        decode(&mixed, 16)
            .unwrap_err()
            .to_string()
            .contains("chain changes source rate")
    );
    let same = [
        vorbis_fixture::link(vorbis_fixture::silence(1, 1), 101, 24_000),
        vorbis_fixture::link(vorbis_fixture::silence(1, 1), 202, 24_000),
    ]
    .concat();
    assert_eq!(decode(&same, 8).unwrap().frames(), 2);
}

#[test]
fn structural_truncation_and_declared_metadata_refuse_before_codec_payload() {
    let wav = wav(24_000, 1, &[1, 2]);
    let flac = flac_fixture::flac16(24_000, 1, &[1, 2], Some(2));
    let ogg = vorbis_fixture::silence(1, 48);
    let mp3 = mp3_fixture::silence(1, 2);
    let cases = [
        (
            "WAV truncated chunk",
            wav[..wav.len() - 1].to_vec(),
            "malformed",
        ),
        (
            "FLAC truncated metadata",
            flac[..41].to_vec(),
            "truncated FLAC metadata body",
        ),
        (
            "Ogg truncated page",
            ogg[..ogg.len() - 1].to_vec(),
            "truncated Ogg page body",
        ),
        (
            "MP3 truncated frame",
            mp3[..mp3.len() - 1].to_vec(),
            "truncated MPEG frame",
        ),
    ];
    for (name, bytes, expected) in cases {
        let error = decode(&bytes, 8192).unwrap_err().to_string();
        assert!(
            error.to_lowercase().contains(&expected.to_lowercase()),
            "{name}: {error}"
        );
    }
    let declared_flac = flac_fixture::flac16(24_000, 1, &[1, 2], Some(1u64 << 35));
    assert!(
        decode(&declared_flac, 8)
            .unwrap_err()
            .to_string()
            .contains("declared FLAC PCM")
    );
    let mut declared_ogg = ogg;
    let last = vorbis_fixture::pages(&declared_ogg).last().unwrap().0;
    declared_ogg[last + 6..last + 14].copy_from_slice(&1_000_000u64.to_le_bytes());
    vorbis_fixture::reseal_page(&mut declared_ogg, last);
    assert!(
        decode(&declared_ogg, 8192)
            .unwrap_err()
            .to_string()
            .contains("final granule exceeds")
    );
}

#[test]
fn large_ignored_flac_and_wav_metadata_preserve_pcm_but_malformed_lengths_reject() {
    let original = flac_fixture::flac16(8000, 1, &[16_384], Some(1));
    let payload = vec![0xa5; 128 * 1024];
    let mut flac = original.clone();
    flac[4] = 0;
    let mut block = vec![0x84, 0x02, 0x00, 0x00];
    block.extend_from_slice(&payload);
    flac.splice(42..42, block);
    assert_eq!(decode(&flac, 4).unwrap().samples(), [0.5]);
    let mut malformed = flac;
    malformed[44] = 0x03;
    assert!(
        decode(&malformed, 4)
            .unwrap_err()
            .to_string()
            .contains("truncated FLAC metadata body")
    );

    let original = wav(8000, 1, &[16_384]);
    let mut with_junk = original[..36].to_vec();
    with_junk.extend_from_slice(b"JUNK");
    with_junk.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    with_junk.extend_from_slice(&payload);
    with_junk.extend_from_slice(&original[36..]);
    let riff_bytes = (with_junk.len() - 8) as u32;
    with_junk[4..8].copy_from_slice(&riff_bytes.to_le_bytes());
    assert_eq!(decode(&with_junk, 4).unwrap().samples(), [0.5]);
    with_junk[40..44].copy_from_slice(&((payload.len() + 1) as u32).to_le_bytes());
    assert!(decode(&with_junk, 4).is_err());
}

fn vorbis_with_vendor(mut bytes: Vec<u8>, vendor_size: usize) -> Vec<u8> {
    let pages = vorbis_fixture::pages(&bytes);
    let (offset, old_size) = pages[1];
    let mut packet = b"\x03vorbis".to_vec();
    packet.extend_from_slice(&(vendor_size as u32).to_le_bytes());
    packet.extend(std::iter::repeat_n(b'v', vendor_size));
    packet.extend_from_slice(&0u32.to_le_bytes());
    packet.push(1);
    let segments = packet.len() / 255 + 1;
    assert!(segments <= 255);
    let mut page = bytes[offset..offset + 27].to_vec();
    page[26] = segments as u8;
    page.extend(std::iter::repeat_n(255, segments - 1));
    page.push((packet.len() % 255) as u8);
    page.extend_from_slice(&packet);
    vorbis_fixture::reseal_page(&mut page, 0);
    bytes.splice(offset..offset + old_size, page);
    bytes
}

#[test]
fn large_mp3_id3_and_vorbis_comments_preserve_audio_and_malformed_declarations_reject() {
    let audio = mp3_fixture::silence(1, 1);
    let mut mp3 = b"ID3\x04\0\0\0\x04\0\0".to_vec(); // 65,536 synchsafe bytes.
    mp3.resize(mp3.len() + 65_536, 0);
    mp3.extend_from_slice(&audio);
    assert_eq!(decode(&mp3, 2304).unwrap().frames(), 576);
    let chart = one_chart("tone.mp3");
    let files = memory(&chart, &[("tone.mp3", &mp3)]);
    assert_eq!(
        prepare(
            &files,
            &chart,
            AudioFormat::new(48_000, 1).unwrap(),
            limits(2304, 2304, 1),
            ChannelPolicy::Exact
        )
        .unwrap()
        .bank
        .total_bytes(),
        2304
    );
    let mut malformed_mp3 = mp3;
    malformed_mp3[6..10].copy_from_slice(&[1, 0, 0, 0]); // Declares 2 MiB, beyond this file.
    assert!(
        decode(&malformed_mp3, 2304)
            .unwrap_err()
            .to_string()
            .contains("truncated ID3 body")
    );

    let vorbis = vorbis_with_vendor(vorbis_fixture::silence(1, 48), 8192);
    assert_eq!(decode(&vorbis, 192).unwrap().frames(), 48);
    let chart = one_chart("tone.ogg");
    let files = memory(&chart, &[("tone.ogg", &vorbis)]);
    assert_eq!(
        prepare(
            &files,
            &chart,
            AudioFormat::new(48_000, 1).unwrap(),
            limits(192, 192, 1),
            ChannelPolicy::Exact
        )
        .unwrap()
        .bank
        .total_bytes(),
        192
    );
    let mut malformed_vorbis = vorbis;
    let comment = vorbis_fixture::pages(&malformed_vorbis)[1].0;
    let body = comment + 27 + usize::from(malformed_vorbis[comment + 26]);
    malformed_vorbis[body + 7..body + 11].copy_from_slice(&u32::MAX.to_le_bytes());
    vorbis_fixture::reseal_page(&mut malformed_vorbis, comment);
    let error = decode(&malformed_vorbis, 192).unwrap_err().to_string();
    assert!(
        !error.contains("CRC") && !error.contains("truncated Ogg"),
        "parsed comment declaration must fail after page preflight: {error}"
    );
}

#[test]
fn wav_nonfinite_float_rejects_with_explicit_preflight_evidence() {
    let mut bytes = wav(24_000, 1, &[0, 0]);
    bytes[20..22].copy_from_slice(&3u16.to_le_bytes());
    bytes[28..32].copy_from_slice(&(24_000u32 * 4).to_le_bytes());
    bytes[32..34].copy_from_slice(&4u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&32u16.to_le_bytes());
    bytes[40..44].copy_from_slice(&4u32.to_le_bytes());
    bytes.truncate(48);
    bytes[44..48].copy_from_slice(&f32::NAN.to_le_bytes());
    bytes[4..8].copy_from_slice(&40u32.to_le_bytes());
    let error = decode(&bytes, 4).unwrap_err();
    assert!(
        error.to_string().to_lowercase().contains("finite"),
        "{error}"
    );
}

#[test]
fn late_flac_frame_crc_failure_follows_a_valid_decoded_frame() {
    let mut first = flac_fixture::flac16(8000, 1, &[1; 16], Some(17));
    let mut second = flac_fixture::flac16(8000, 1, &[2], None);
    second[46] = 1;
    flac_fixture::refresh_flac16_checksums(&mut second);
    first.extend_from_slice(&second[42..]);
    assert_eq!(decode(&first, 68).unwrap().frames(), 17);
    let last = first.len() - 1;
    first[last] ^= 1; // Only the second frame's CRC16 is invalid.
    let error = decode(&first, 68).unwrap_err().to_string();
    assert!(
        error.to_lowercase().contains("crc") || error.to_lowercase().contains("checksum"),
        "late FLAC frame error: {error}"
    );
}

#[test]
fn late_vorbis_audio_packet_failure_survives_resealed_ogg_preflight() {
    let mut bytes = vorbis_fixture::silence(1, 48);
    assert_eq!(decode(&bytes, 192).unwrap().frames(), 48);
    let pages = vorbis_fixture::pages(&bytes);
    let late = pages[pages.len() - 1].0;
    let body = late + 27 + usize::from(bytes[late + 26]);
    bytes[body] = 0xff; // Audio packet flag becomes an invalid Vorbis packet type.
    vorbis_fixture::reseal_page(&mut bytes, late);
    let error = decode(&bytes, 192).unwrap_err();
    let stage = error.downcast_ref::<lewton::audio::AudioReadError>();
    assert!(stage.is_some(), "Ogg preflight must pass: {error}");
    assert_eq!(stage, Some(&lewton::audio::AudioReadError::AudioIsHeader));
}

#[test]
fn late_mp3_payload_failure_survives_complete_frame_preflight() {
    let mut bytes = mp3_fixture::silence(1, 3);
    assert_eq!(decode(&bytes, 3 * 576 * 4).unwrap().frames(), 3 * 576);
    bytes[192 + 4] = 255; // Late frame requests more reservoir bytes than the first frame carries.
    let error = match decode(&bytes, 3 * 576 * 4) {
        Ok(_) => panic!("late MP3 reservoir corruption unexpectedly decoded"),
        Err(error) => error.to_string(),
    };
    assert!(
        !error.contains("truncated")
            && !error.contains("source format")
            && !error.contains("synchronization"),
        "MPEG preflight must pass: {error}"
    );
    assert!(
        error.contains("bit reservoir data"),
        "late MP3 payload error: {error}"
    );
}

#[test]
fn earlier_valid_asset_then_malformed_asset_returns_no_bank_and_retry_is_unchanged() {
    let chart = b"#BPM 120\n#WAV01 good.wav\n#WAV02 later.mp3\n#00011:0102\n";
    let good = wav(24_000, 1, &[16_384]);
    let mut bad = mp3_fixture::silence(1, 2);
    bad[192 + 4] = 255;
    let files = memory(chart, &[("good.wav", &good), ("later.mp3", &bad)]);
    let output = AudioFormat::new(48_000, 1).unwrap();
    let cap = limits(4608, 4612, 2);
    let failure = match prepare(&files, chart, output, cap, ChannelPolicy::Exact) {
        Ok(_) => panic!("malformed second asset unexpectedly prepared"),
        Err(error) => error.to_string(),
    };
    assert!(
        !failure.contains("good.wav"),
        "late failure after the first valid asset: {failure}"
    );
    let valid = mp3_fixture::silence(1, 2);
    let repaired = memory(chart, &[("good.wav", &good), ("later.mp3", &valid)]);
    for _ in 0..2 {
        let prepared = prepare(&repaired, chart, output, cap, ChannelPolicy::Exact).unwrap();
        assert_eq!(prepared.bank.total_bytes(), 4612);
        assert_eq!(prepared.bank.get(SampleId(1)).unwrap().samples(), &[0.5]);
        assert_eq!(prepared.bank.get(SampleId(2)).unwrap().frames(), 1152);
    }
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..1000 {
            let path = std::env::temp_dir().join(format!(
                "beatkernel-media-corpus-{}-{}",
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

#[test]
fn actual_filesystem_prepares_supported_audio_then_rejects_sparse_oversized_asset_before_decoder() {
    let dir = Directory::new();
    let format = AudioFormat::new(48_000, 1).unwrap();
    for (codec, encoded, rate, frames, _) in mono_cases() {
        dir.write("tone.bin", &encoded);
        let chart = dir.write("chart.bms", &one_chart("tone.bin"));
        let prepared = load_prepared(
            &chart,
            format,
            limits(frames * 4, frames * 4, 1),
            ChannelPolicy::Exact,
        )
        .unwrap();
        assert_eq!(
            prepared.bank.get(SampleId(1)).unwrap().format(),
            AudioFormat::new(rate, 1).unwrap(),
            "{codec}"
        );
    }
    let sparse = dir.0.join("tone.bin");
    std::fs::File::create(&sparse)
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    let chart = dir.write("chart.bms", &one_chart("tone.bin"));
    let error = load_prepared(&chart, format, limits(8, 8, 1), ChannelPolicy::Exact)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("encoded file exceeds preparation limit"),
        "sparse encoded cap must reject at bounded read: {error}"
    );
}
