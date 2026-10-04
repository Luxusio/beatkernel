//! Deferred real shared preparation/section fixtures; no filesystem or output device.
use crate::{
    AssetDecoder, ChannelPolicy, DEFAULT_BMS_PCM_SAMPLES, PreparedBms,
    asset_paths::AssetPathPolicy,
    asset_source::{MemoryAssetLimits, MemoryFiles},
    prepare_from_source, section_start,
};
use beatkernel::{
    audio::{AudioCommand, AudioFormat, PcmLimits, PcmSample, SampleId},
    time::Timestamp,
};
use std::{cell::Cell, error::Error, fmt::Write, path::Path};

#[derive(Default)]
struct TwoFrames {
    calls: Cell<usize>,
}
impl AssetDecoder for TwoFrames {
    fn decode(
        &self,
        path: &Path,
        encoded: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(path, Path::new("song/shared.pcm"));
        assert_eq!(encoded, b"two original frames");
        Ok(PcmSample::new(
            AudioFormat::new(10, 1)?,
            vec![0.25, 0.5],
            limits,
        )?)
    }
}

fn full_namespace(extra_cues: usize) -> String {
    // Literal alphabet from the format contract, not the parser's radix helper.
    let alphabet = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut text = String::from("#BASE 62\n#BPM 60\n");
    for &first in alphabet {
        for &second in alphabet {
            if first == b'0' && second == b'0' {
                continue;
            }
            let (first, second) = (char::from(first), char::from(second));
            writeln!(
                text,
                "#WAV{first}{second} shared.pcm\n#00001:{first}{second}"
            )
            .unwrap();
        }
    }
    for _ in 0..extra_cues {
        text.push_str("#00001:01\n");
    }
    text.push_str("#00111:zz\n"); // Future keysound remains at the original four-second target.
    text
}

fn prepare(
    text: &str,
    limits: PcmLimits,
    decoder: &TwoFrames,
) -> Result<PreparedBms, Box<dyn Error>> {
    let mut files = MemoryFiles::new(MemoryAssetLimits::default())?;
    files.insert("song/chart.bms", text.as_bytes().to_vec())?;
    files.insert("song/shared.pcm", b"two original frames".to_vec())?;
    let assets = files.scope("song/chart.bms")?;
    prepare_from_source(
        text.as_bytes(),
        &assets,
        AudioFormat::new(10, 1)?,
        limits,
        ChannelPolicy::Exact,
        decoder,
        AssetPathPolicy::Exact,
        0,
        None,
    )
}

#[test]
fn all_nonzero_base62_originals_and_4096_real_crossing_cues_keep_exact_keysound_and_suffix_identities()
 {
    assert_eq!(DEFAULT_BMS_PCM_SAMPLES, 3844);
    let limits =
        PcmLimits::new(64 * 1024 * 1024, 256 * 1024 * 1024, DEFAULT_BMS_PCM_SAMPLES).unwrap();
    let decoder = TwoFrames::default();
    let original = prepare(&full_namespace(253), limits, &decoder).unwrap();
    assert_eq!(
        decoder.calls.get(),
        1,
        "canonical aliases share decoding, while each ID owns admitted PCM"
    );
    assert_eq!(
        (original.bank.len(), original.bank.total_bytes()),
        (3843, 30744)
    );
    assert_eq!(original.source.samples.len(), 3843);
    assert!(original.bank.get(SampleId(0)).is_none());
    assert_eq!(
        (original.compiled.bgm.len(), original.bgm_commands.len()),
        (4096, 4096)
    );
    assert!(
        original
            .compiled
            .bgm
            .iter()
            .all(|cue| cue.at == Timestamp::ZERO)
    );
    assert_eq!(original.source.notes[0].sample, SampleId(3843));
    assert_eq!(
        original.compiled.chart.objects()[0].time.start.as_nanos(),
        4_000_000_000
    );
    let object = original.source.notes[0].object;
    let (section, report) =
        section_start::prepare_at(original, Timestamp::from_nanos(100_000_000), limits).unwrap();
    assert_eq!(
        (section.bank.len(), section.bank.total_bytes()),
        (7939, 47128)
    );
    assert_eq!(report.tails.len(), 4096);
    assert_eq!(
        (
            report.excluded_objects,
            report.excluded_crossing_holds,
            report.retired_bgm
        ),
        (0, 0, 0)
    );
    for (index, source, suffix, voice) in [
        (0, 1, 3844, 2),
        (3842, 3843, 7686, 3844),
        (3843, 1, 7687, 3845),
        (4095, 1, 7939, 4097),
    ] {
        let tail = &report.tails[index];
        assert_eq!(
            (tail.source.0, tail.suffix.0, tail.voice.0, tail.frame),
            (source, suffix, voice, 1)
        );
        assert_eq!(
            (tail.applied_song_time.as_nanos(), tail.correction_ns),
            (100_000_000, 0)
        );
        assert_eq!(
            section.bgm_commands[index],
            AudioCommand::Play {
                voice: tail.voice,
                sample: SampleId(suffix),
                at: Timestamp::from_nanos(100_000_000),
                gain: 1.0,
            }
        );
    }
    for id in [1, 10, 36, 1296, 3843] {
        let pcm = section.bank.get(SampleId(id)).unwrap();
        assert_eq!((pcm.format().sample_rate(), pcm.frames()), (10, 2));
        assert_eq!(pcm.samples(), [0.25, 0.5]);
    }
    for id in [3844, 7686, 7687, 7939] {
        let pcm = section.bank.get(SampleId(id)).unwrap();
        assert_eq!(pcm.frames(), 1);
        assert_eq!(pcm.samples(), [0.5]);
    }
    assert!(section.bank.get(SampleId(7940)).is_none());
    assert_eq!(section.source.notes[0].object, object);
    assert_eq!(section.sounds.len(), 1);
    assert_eq!(
        (section.sounds[0].object, section.sounds[0].sample),
        (object, SampleId(3843))
    );
    assert_eq!(
        section.compiled.chart.objects()[0].time.start.as_nanos(),
        4_000_000_000
    );
    assert_eq!(
        decoder.calls.get(),
        1,
        "section suffixes copy original PCM without re-decoding"
    );
}

#[test]
fn caller_count_limits_refuse_before_decode_and_aliases_still_pay_each_originals_pcm_bytes() {
    let text = full_namespace(0);
    for count in [1295, 3842] {
        let decoder = TwoFrames::default();
        let error = prepare(&text, PcmLimits::new(8, 30744, count).unwrap(), &decoder).unwrap_err();
        assert!(error.to_string().contains("referenced asset count"));
        assert_eq!(decoder.calls.get(), 0);
    }
    let decoder = TwoFrames::default();
    let exact = prepare(&text, PcmLimits::new(8, 30744, 3843).unwrap(), &decoder).unwrap();
    assert_eq!((exact.bank.len(), exact.bank.total_bytes()), (3843, 30744));
    assert_eq!(decoder.calls.get(), 1);
    for limits in [
        PcmLimits::new(4, 30744, 3844).unwrap(),
        PcmLimits::new(8, 30743, 3844).unwrap(),
    ] {
        let decoder = TwoFrames::default();
        assert!(prepare(&text, limits, &decoder).is_err());
        assert_eq!(
            decoder.calls.get(),
            1,
            "adequate count capacity does not waive asset or aggregate bytes"
        );
    }
    assert_eq!(
        exact.bank.get(SampleId(3843)).unwrap().samples(),
        [0.25, 0.5]
    );
}

#[test]
fn suffix_byte_budget_and_4096_cue_cap_remain_independent_of_the_expanded_original_namespace() {
    let text = full_namespace(253);
    let decoder = TwoFrames::default();
    let too_small = PcmLimits::new(8, 47127, 3844).unwrap();
    let original = prepare(&text, too_small, &decoder).unwrap();
    assert_eq!(original.bank.total_bytes(), 30744);
    assert!(
        section_start::prepare_at(original, Timestamp::from_nanos(100_000_000), too_small).is_err()
    );
    let exact = PcmLimits::new(8, 47128, 3844).unwrap();
    let original = prepare(&text, exact, &decoder).unwrap();
    let (section, report) =
        section_start::prepare_at(original, Timestamp::from_nanos(100_000_000), exact).unwrap();
    assert_eq!(
        (report.tails.len(), section.bank.total_bytes()),
        (4096, 47128)
    );
    let roomy = PcmLimits::new(8, 100_000, 3844).unwrap();
    let excess = prepare(&full_namespace(254), roomy, &decoder).unwrap();
    assert_eq!(excess.bgm_commands.len(), 4097);
    assert!(
        section_start::prepare_at(excess, Timestamp::from_nanos(100_000_000), roomy)
            .unwrap_err()
            .to_string()
            .contains("4096")
    );
    let untouched = prepare(&text, roomy, &decoder).unwrap();
    let (zero, report) = section_start::prepare_at(untouched, Timestamp::ZERO, roomy).unwrap();
    assert_eq!((zero.bank.len(), zero.bank.total_bytes()), (3843, 30744));
    assert!(report.tails.is_empty());
    assert!(zero.bgm_commands.iter().all(|command| matches!(
        command,
        AudioCommand::Play {
            at: Timestamp::ZERO,
            ..
        }
    )));
}
