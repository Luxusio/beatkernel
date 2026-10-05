//! Deferred actual source admission. A mine timeline is not playable until the
//! shared hazard, gauge, sound, rendering and replay owners are integrated.
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
        self.resolved.borrow_mut().push(name.into());
        assert_eq!(policy, AssetPathPolicy::Exact);
        assert_eq!(name, "sample.pcm");
        Ok(name.into())
    }
    fn read<'a>(&'a self, path: &Path, bound: usize) -> io::Result<Cow<'a, [u8]>> {
        self.reads.set(self.reads.get() + 1);
        assert_eq!(path, Path::new("sample.pcm"));
        assert_eq!(bound, 64 * 1024 * 1024);
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
        bytes: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(path, Path::new("sample.pcm"));
        assert_eq!(bytes, [1, 2, 3]);
        Ok(PcmSample::new(
            AudioFormat::new(10, 1)?,
            vec![0.25, -0.5],
            limits,
        )?)
    }
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
    let replay_limits = ReplayCodecLimits::new(4096, 16, 2048, CodecLimits::new(4096, 1024)?)?;
    prepare_from_source(
        text.as_bytes(),
        assets,
        AudioFormat::new(10, 1)?,
        PcmLimits::new(8, 16, 4)?,
        ChannelPolicy::Exact,
        decoder,
        AssetPathPolicy::Exact,
        0,
        replay.map(|file| (file, replay_limits)),
    )
}
fn no_assets(assets: &Assets, decoder: &Decoder) {
    assert!(assets.resolved.borrow().is_empty());
    assert_eq!(assets.reads.get(), 0);
    assert_eq!(decoder.calls.get(), 0);
}

#[test]
fn parsed_nonempty_mines_refuse_before_replay_or_assets_without_masking_original_parser_errors() {
    let replay = invalid_replay();
    for mine in [
        "#000D1:01",
        "#000E9:ZZ",
        "#000D1:001E00",
        "#000D1:zz\n#BASE 16",
        "#WAV00 absent-explosion.pcm\n#000D6:01",
    ] {
        let text = format!("#BPM 60\n#WAV01 sample.pcm\n#00011:01\n#00001:0001\n{mine}");
        for recording in [None, Some(&replay)] {
            let assets = Assets::default();
            let decoder = Decoder::default();
            let error = prepare(&text, &assets, &decoder, recording).unwrap_err();
            assert!(error.to_string().contains("mine"));
            assert!(
                error.downcast_ref::<BmsError>().is_none()
                    && error.downcast_ref::<PlaybackError>().is_none()
            );
            no_assets(&assets, &decoder);
        }
    }
    let assets = Assets::default();
    let decoder = Decoder::default();
    let error = prepare("#000D1:01\n#00011:02", &assets, &decoder, Some(&replay)).unwrap_err();
    let parser = error.downcast_ref::<BmsError>().unwrap();
    assert_eq!(
        (parser.line, &parser.kind),
        (
            2,
            &BmsErrorKind::MissingDefinition {
                kind: "WAV",
                index: 2
            }
        )
    );
    no_assets(&assets, &decoder);
    let error = prepare("#000D1:01\n#VOLWAV bad", &assets, &decoder, Some(&replay)).unwrap_err();
    assert_eq!(error.downcast_ref::<BmsError>().unwrap().line, 2);
    no_assets(&assets, &decoder);
    let error = prepare(
        "#WAV01 sample.pcm\n#00011:01\n#000D1:$1",
        &assets,
        &decoder,
        None,
    )
    .unwrap_err();
    assert_eq!(error.downcast_ref::<BmsError>().unwrap().line, 3);
    no_assets(&assets, &decoder);
    let error = prepare(
        "#WAV01 sample.pcm\n#00011:01",
        &assets,
        &decoder,
        Some(&replay),
    )
    .unwrap_err();
    assert!(error.downcast_ref::<PlaybackError>().is_some());
    no_assets(&assets, &decoder);
}

#[test]
fn empty_rest_and_inactive_mines_preserve_actual_ordinary_and_invisible_pcm_admission_without_wav00_loading()
 {
    let prefix = "#BPM 60\n#WAV01 sample.pcm\n#00011:01\n#00001:0001\n#00031:01\n";
    let plain = prepare(prefix, &Assets::default(), &Decoder::default(), None).unwrap();
    for suffix in [
        "#000D1:00000000000000\n#000E9:00",
        "#SETRANDOM 1\n#IF 2\n#BASE invalid\n#000D1:INVALID\n#WAV00 absent-explosion.pcm\n#ENDIF",
        "#WAV00 absent-explosion.pcm\n#000D6:00",
    ] {
        let assets = Assets::default();
        let decoder = Decoder::default();
        let prepared = prepare(&format!("{prefix}{suffix}"), &assets, &decoder, None).unwrap();
        assert!(
            prepared.source.mines.is_empty() && prepared.source.compile_mines().unwrap().is_empty()
        );
        assert_eq!(
            prepared.source.mine_ticks_per_beat,
            prepared.source.source.ticks_per_beat
        );
        assert_eq!(prepared.source.source, plain.source.source);
        assert_eq!(prepared.source.notes, plain.source.notes);
        assert_eq!(prepared.source.invisible, plain.source.invisible);
        assert_eq!(prepared.compiled, plain.compiled);
        assert_eq!((prepared.bank.len(), prepared.bank.total_bytes()), (1, 8));
        assert_eq!(
            prepared.bank.get(SampleId(1)).unwrap().samples(),
            [0.25, -0.5]
        );
        assert_eq!((prepared.sounds.len(), prepared.bgm_commands.len()), (1, 1));
        assert_eq!(prepared.compiled.bgm[0].at.as_nanos(), 2_000_000_000);
        assert_eq!(*assets.resolved.borrow(), ["sample.pcm"]);
        assert_eq!((assets.reads.get(), decoder.calls.get()), (1, 1));
    }
    let assets = Assets::default();
    let decoder = Decoder::default();
    let empty = prepare(
        "#WAV00 absent-explosion.pcm\n#000D1:000000\n#000E9:00",
        &assets,
        &decoder,
        None,
    )
    .unwrap();
    assert!(empty.source.mines.is_empty() && empty.compiled.chart.objects().is_empty());
    assert_eq!(empty.bank.len(), 0);
    assert!(empty.sounds.is_empty() && empty.bgm_commands.is_empty());
    no_assets(&assets, &decoder);
}
