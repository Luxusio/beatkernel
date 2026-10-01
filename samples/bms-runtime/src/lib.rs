//! Off-thread bounded BMS preparation shared by offline and native compositions.
#![forbid(unsafe_code)]
/// Rolling BGM admission on an explicitly configured output frame grid.
pub mod bgm;
/// Saved-record opponents and actual judgment summaries.
pub mod competition;
/// Application competition options and native runtime observation.
pub mod competition_live;
/// Actual judge completion and native output drain for full-song play.
pub mod completion;
/// Portable bounded audio device metadata and explicit draft selection.
pub mod device_catalog;
/// Original bitmap glyph atlas data, prepared outside rendering callbacks.
#[cfg(feature = "graphics")]
pub mod font;
/// Reusable asynchronous native/Web GPU presentation, separate from game I/O.
#[cfg(feature = "graphics")]
pub mod graphics;
/// Bounded native input merging on one common host clock.
pub mod local_input;
/// Collection-based local player identity and unique native input assignment.
pub mod local_players;
/// Shared-transport/output execution over independent actual core runtimes.
pub mod local_runtime;
/// Graphical local-player draft using typed keyboard metadata and stable IDs.
pub mod local_setup;
/// Bounded two-player progress exchange on a dedicated socket worker.
pub mod multiplayer;
/// Omitted solo option defaults, independent of native discovery.
pub mod native_defaults;
/// Synthetic offline composition using the same runtime and mixer as native apps.
pub mod offline;
/// Typed UI panel ownership and cancellation permits for off-thread work.
pub mod panel_scope;
/// Native presentation-derived pause and bounded keyboard reconciliation.
pub mod playback_pause;
/// Actual game-to-UI presentation and cancellation outside audio callbacks.
pub mod player;
/// Bounded chart catalog and exact compiled lane display data.
pub mod player_chart;
#[cfg(feature = "graphics")]
mod playfield_gpu;
/// Exact original-song practice positions and native-setting draft updates.
pub mod practice;
/// Portable display configuration shared by CLI, graphical drafts and profiles.
pub mod presentation_settings;
/// Bounded saved-record discovery and chart/profile-compatible prefix previews.
pub mod record_catalog;
/// Song-time command planning from actual recorded BMS judgment.
pub mod replay_audio;
/// Bounded capture of the actual native runtime's accepted judgment operations.
pub mod replay_capture;
pub mod replay_pause;
/// Checked durable replay reconstruction through the same builtin BMS judge.
pub mod replay_playback;
/// Bounded PCM rendering of captured BMS play through the actual core Mixer.
pub mod replay_render;
/// Incremental presentation of validated recorded judging operations.
pub mod replay_visual;
/// Bounded platform-independent geometry for GPU presentation.
#[cfg(feature = "graphics")]
pub mod scene;
/// Typed screen route and lifecycle admission independent of a UI toolkit.
pub mod screen_lifecycle;
/// Original-timeline chart and PCM preparation for fresh practice starts.
pub mod section_start;
/// Immutable native invocations and fresh-session retry naming.
pub mod session_launch;
/// Bounded native option drafts for off-thread application configuration.
pub mod settings;
/// Versioned native settings profiles and bounded off-thread file storage.
pub mod settings_profile;
/// Validated portable raw texture resources.
#[cfg(feature = "graphics")]
pub mod texture;
/// Atomic Design-style presentation compositions, independent of native I/O.
#[cfg(feature = "graphics")]
pub mod ui;

use beatkernel::{
    audio::{AudioCommand, AudioFormat, PcmLimits, PcmSample, SampleBank, VoiceId},
    judge::JudgeStage,
    runtime::SoundBinding,
};
use beatkernel_bms::{BmsChart, CompiledBms, ParseOptions, parse};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
};

/// Fully prepared chart and owned assets; all timestamps remain in song time.
#[derive(Debug)]
pub struct PreparedBms {
    /// Parsed source and adapter-owned provenance, lanes and asset names.
    pub source: BmsChart,
    /// Actual core-compiled gameplay and BGM timeline.
    pub compiled: CompiledBms,
    /// PCM bank with the explicit caller-selected output format.
    pub bank: SampleBank,
    /// Gameplay head/instant mappings, using ObjectId values as voice identities.
    pub sounds: Vec<SoundBinding>,
    /// Song-relative BGM commands; the caller must map timestamps before admission.
    pub bgm_commands: Vec<AudioCommand>,
}

/// Explicit supported channel treatment; no layout inference is performed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelPolicy {
    /// Require the decoded channel count to match the requested output exactly.
    Exact,
    /// Also permit mono to stereo, preserving amplitude in both channels.
    MonoToStereo,
}

/// Caller-supplied off-thread codec boundary, after path and encoded-byte checks.
///
/// Implementations must respect the supplied PCM limits. Returned PCM is checked
/// again at bank admission; this trait cannot bound a custom decoder's own work.
pub trait AssetDecoder {
    /// Decode resolved asset bytes into finite interleaved PCM without native IO.
    fn decode(
        &self,
        path: &Path,
        encoded: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>>;
}

/// Default strict RIFF WAVE decoder; no additional formats are implied.
#[derive(Clone, Copy, Debug, Default)]
pub struct WavDecoder;
impl AssetDecoder for WavDecoder {
    fn decode(
        &self,
        _path: &Path,
        encoded: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>> {
        Ok(PcmSample::from_wav(encoded, limits)?)
    }
}

/// Prepare a bounded UTF-8 BMS chart and its referenced WAV assets off-thread.
pub fn load_prepared(
    path: &Path,
    format: AudioFormat,
    pcm_limits: PcmLimits,
    channels: ChannelPolicy,
) -> Result<PreparedBms, Box<dyn Error>> {
    load_prepared_with_decoder(path, format, pcm_limits, channels, &WavDecoder)
}

/// Prepare using an explicit codec, retaining the same path and storage policy.
///
/// Encoded assets are bounded to 64 MiB before decoder invocation. This function
/// only produces song-time commands; it does not schedule or enqueue them.
pub fn load_prepared_with_decoder(
    path: &Path,
    format: AudioFormat,
    pcm_limits: PcmLimits,
    channels: ChannelPolicy,
    decoder: &dyn AssetDecoder,
) -> Result<PreparedBms, Box<dyn Error>> {
    let chart_path = std::fs::canonicalize(path)?;
    let root = chart_path.parent().ok_or("chart has no parent")?;
    let options = ParseOptions::default();
    let encoded_chart = bounded_read(&chart_path, options.max_bytes)?;
    let source = parse(std::str::from_utf8(&encoded_chart)?, options)?;
    let compiled = source.compile()?;
    let referenced: BTreeSet<_> = source
        .notes
        .iter()
        .map(|note| note.sample)
        .chain(compiled.bgm.iter().map(|event| event.sample))
        .collect();
    if referenced.len() > pcm_limits.max_samples() {
        return Err("referenced asset count exceeds PCM limits".into());
    }
    let mut bank = SampleBank::new(format, pcm_limits)?;
    for sample in referenced {
        let name = source
            .samples
            .get(&u16::try_from(sample.0)?)
            .ok_or("referenced sample has no WAV definition")?;
        let asset_path = resolve_asset(root, name)?;
        let encoded = bounded_read(&asset_path, 64 * 1024 * 1024)?;
        let pcm = decoder.decode(&asset_path, &encoded, pcm_limits)?;
        let pcm = prepare_channels(pcm, format, pcm_limits, channels)?;
        bank.insert(sample, pcm)?;
    }

    let objects: BTreeMap<_, _> = compiled
        .chart
        .objects()
        .iter()
        .map(|object| (object.id, object))
        .collect();
    let mut sounds = Vec::new();
    sounds.try_reserve_exact(source.notes.len())?;
    for note in &source.notes {
        let object = objects
            .get(&note.object)
            .ok_or("note has no compiled object")?;
        sounds.push(SoundBinding {
            object: note.object,
            stage: if object.time.end.is_some() {
                JudgeStage::HoldHead
            } else {
                JudgeStage::Instant
            },
            sample: note.sample,
            voice: VoiceId(note.object.0),
            gain: 1.0,
        });
    }
    let mut bgm_commands = Vec::new();
    bgm_commands.try_reserve_exact(compiled.bgm.len())?;
    if !compiled.bgm.is_empty() {
        let first_voice = objects
            .keys()
            .map(|id| id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("BGM voice identity overflow")?;
        for (index, event) in compiled.bgm.iter().enumerate() {
            let voice = first_voice
                .checked_add(u64::try_from(index)?)
                .ok_or("BGM voice identity overflow")?;
            bgm_commands.push(AudioCommand::Play {
                voice: VoiceId(voice),
                sample: event.sample,
                at: event.at,
                gain: 1.0,
            });
        }
    }
    Ok(PreparedBms {
        source,
        compiled,
        bank,
        sounds,
        bgm_commands,
    })
}

fn bounded_read(path: &Path, limit: usize) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    let mut block = [0u8; 8192];
    loop {
        let count = file.read(&mut block)?;
        if count == 0 {
            break;
        }
        let length = bytes
            .len()
            .checked_add(count)
            .ok_or("encoded length overflow")?;
        if length > limit {
            return Err("encoded file exceeds preparation limit".into());
        }
        bytes.try_reserve(count)?;
        bytes.extend_from_slice(&block[..count]);
    }
    Ok(bytes)
}

fn resolve_asset(root: &Path, name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let portable = name.replace('\\', "/");
    if portable.as_bytes().get(1) == Some(&b':') {
        return Err("drive-qualified asset path rejected".into());
    }
    let relative = Path::new(&portable);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err("absolute/parent asset path rejected".into());
    }
    let resolved = std::fs::canonicalize(root.join(relative))?;
    if !resolved.starts_with(root) {
        return Err("asset symlink escapes chart directory".into());
    }
    Ok(resolved)
}

fn prepare_channels(
    pcm: PcmSample,
    output: AudioFormat,
    limits: PcmLimits,
    policy: ChannelPolicy,
) -> Result<PcmSample, Box<dyn Error>> {
    let source = pcm.format();
    if source.channels() == output.channels() {
        return Ok(pcm);
    }
    if policy != ChannelPolicy::MonoToStereo || source.channels() != 1 || output.channels() != 2 {
        return Err("decoded channel count does not match explicit output policy".into());
    }
    let length = pcm
        .samples()
        .len()
        .checked_mul(2)
        .ok_or("stereo expansion length overflow")?;
    let bytes = length
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or("stereo expansion bytes overflow")?;
    if bytes > limits.max_asset_bytes() {
        return Err("stereo expansion exceeds PCM asset limit".into());
    }
    let mut expanded = Vec::new();
    expanded.try_reserve_exact(length)?;
    for &sample in pcm.samples() {
        expanded.extend_from_slice(&[sample, sample]);
    }
    Ok(PcmSample::new(
        AudioFormat::new(source.sample_rate(), 2)?,
        expanded,
        limits,
    )?)
}
