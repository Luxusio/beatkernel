//! Deferred common preparation admission, with actual parser/replay and counted asset boundaries.
use crate::{
    AssetDecoder, ChannelPolicy, PreparedBms, asset_paths::AssetPathPolicy,
    asset_source::AssetSource, prepare_from_source, replay_playback::PlaybackError,
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits, PcmSample, SampleId},
    input::CodecLimits,
    replay::{
        ReplayHeader,
        codec::{ReplayCodecLimits, ReplayFile},
    },
    time::ClockDomainId,
};
use beatkernel_bms::{BmsError, BmsErrorKind};
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    error::Error,
    io,
    path::{Path, PathBuf},
};

#[derive(Default)]
struct Assets {
    resolved: RefCell<Vec<String>>,
    reads: Cell<usize>,
}
impl AssetSource for Assets {
    fn resolve(&self, name: &str, policy: AssetPathPolicy) -> io::Result<PathBuf> {
        self.resolved.borrow_mut().push(name.to_owned());
        assert!(matches!(policy, AssetPathPolicy::Exact));
        Ok(PathBuf::from(name))
    }
    fn read<'a>(&'a self, key: &Path, max_bytes: usize) -> io::Result<Cow<'a, [u8]>> {
        self.reads.set(self.reads.get() + 1);
        assert_eq!(key, Path::new("sample.pcm"));
        assert_eq!(max_bytes, 64 * 1024 * 1024);
        Ok(Cow::Borrowed(&[1, 2, 3]))
    }
}
#[derive(Default)]
struct Decoder {
    calls: Cell<usize>,
}
impl AssetDecoder for Decoder {
    fn decode(
        &self,
        path: &Path,
        encoded: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(path, Path::new("sample.pcm"));
        assert_eq!(encoded, [1, 2, 3]);
        Ok(PcmSample::new(
            AudioFormat::new(48_000, 1)?,
            vec![0.25, -0.5],
            limits,
        )?)
    }
}
fn replay_limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(4096, 16, 2048, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn invalid_replay() -> ReplayFile {
    ReplayFile::new(
        ReplayHeader {
            version: 1,
            chart_identity: vec![],
            rules_identity: vec![],
            options: vec![],
            seed: 0,
            normalized_clock: ClockDomainId(1),
        },
        vec![],
    )
}
fn prepare(
    text: &str,
    assets: &Assets,
    decoder: &Decoder,
    replay: Option<&ReplayFile>,
) -> Result<PreparedBms, Box<dyn Error>> {
    prepare_from_source(
        text.as_bytes(),
        assets,
        AudioFormat::new(48_000, 1)?,
        PcmLimits::new(8, 16, 4)?,
        ChannelPolicy::Exact,
        decoder,
        AssetPathPolicy::Exact,
        0,
        replay.map(|file| (file, replay_limits())),
    )
}
fn assert_no_assets(assets: &Assets, decoder: &Decoder) {
    assert!(assets.resolved.borrow().is_empty());
    assert_eq!(assets.reads.get(), 0);
    assert_eq!(decoder.calls.get(), 0);
}

#[test]
fn nonempty_invisible_admission_refuses_before_replay_validation_or_any_asset_boundary() {
    let replay = invalid_replay();
    for row in [
        "#00031:01",
        "#00049:01",
        "#00031:000100",
        "#00031:zz\n#WAVzz invisible.pcm\n#BASE 62",
    ] {
        let text = format!("#BPM 60\n#WAV01 sample.pcm\n#00011:01\n#00001:0001\n{row}");
        for recording in [None, Some(&replay)] {
            let assets = Assets::default();
            let decoder = Decoder::default();
            let error = prepare(&text, &assets, &decoder, recording).unwrap_err();
            assert!(error.to_string().contains("invisible"), "{error}");
            assert!(error.downcast_ref::<PlaybackError>().is_none());
            assert_no_assets(&assets, &decoder);
        }
    }
    let assets = Assets::default();
    let decoder = Decoder::default();
    let error = prepare(
        "#WAV01 sample.pcm\n#00031:02",
        &assets,
        &decoder,
        Some(&replay),
    )
    .unwrap_err();
    let error = error
        .downcast_ref::<BmsError>()
        .expect("the complete parser runs before the admission guard");
    assert_eq!(
        (error.line, &error.kind),
        (
            2,
            &BmsErrorKind::MissingDefinition {
                kind: "WAV",
                index: 2
            }
        )
    );
    assert_no_assets(&assets, &decoder);
    let error = prepare(
        "#WAV01 sample.pcm\n#00011:01",
        &assets,
        &decoder,
        Some(&replay),
    )
    .unwrap_err();
    assert!(
        error.downcast_ref::<PlaybackError>().is_some(),
        "ordinary replay setup still uses the actual shared validator"
    );
    assert_no_assets(&assets, &decoder);
}

#[test]
fn rest_only_and_unselected_invisible_rows_retain_actual_supported_preparation_without_phantom_pcm()
{
    let prefix = "#BPM 60\n#WAV01 sample.pcm\n#00011:01\n#00001:0001\n";
    let plain_assets = Assets::default();
    let plain_decoder = Decoder::default();
    let plain = prepare(prefix, &plain_assets, &plain_decoder, None).unwrap();
    for suffix in [
        "#00031:00000000000000\n#00049:00",
        "#SETRANDOM 1\n#IF 2\n#WAV02 invisible.pcm\n#00031:02\n#00049:INVALID\n#ENDIF",
    ] {
        let assets = Assets::default();
        let decoder = Decoder::default();
        let prepared = prepare(&format!("{prefix}{suffix}"), &assets, &decoder, None).unwrap();
        assert!(prepared.source.invisible.is_empty());
        assert_eq!(prepared.source.source, plain.source.source);
        assert_eq!(prepared.source.notes, plain.source.notes);
        assert_eq!(prepared.compiled, plain.compiled);
        assert_eq!(prepared.source.invisible_ticks_per_beat, 1);
        assert_eq!((prepared.bank.len(), prepared.bank.total_bytes()), (1, 8));
        assert_eq!(
            prepared.bank.get(SampleId(1)).unwrap().samples(),
            [0.25, -0.5]
        );
        assert_eq!(prepared.sounds.len(), 1);
        assert_eq!(prepared.sounds[0].sample, SampleId(1));
        assert_eq!(prepared.bgm_commands.len(), 1);
        assert_eq!(prepared.compiled.bgm[0].at.as_nanos(), 2_000_000_000);
        assert_eq!(prepared.compiled.chart.objects().len(), 1);
        assert_eq!(
            prepared.compiled.chart.objects()[0].time.start.as_nanos(),
            0
        );
        assert_eq!(*assets.resolved.borrow(), ["sample.pcm"]);
        assert_eq!((assets.reads.get(), decoder.calls.get()), (1, 1));
    }
}
