//! Off-thread bounded BMS preparation shared by offline and native compositions.
#![forbid(unsafe_code)]
/// Actual ASIO replay presentation observations retained until their host upper frontier.
pub mod asio_replay;
/// Contained exact or compatible asset filename lookup during preparation.
pub mod asset_paths;
/// Bounded selected-file and native asset acquisition.
pub mod asset_source;
#[cfg(test)]
mod asset_source_fixtures;
/// Prepared original-song image selections shared by live and replay presentation.
pub mod bga;
/// Rolling BGM admission on an explicitly configured output frame grid.
pub mod bgm;
#[cfg(all(target_arch = "wasm32", feature = "browser"))]
pub mod browser;
/// Numeric bindings for the separate AudioWorklet WASM owner.
#[cfg(all(target_arch = "wasm32", feature = "browser-audio"))]
pub mod browser_audio;
#[cfg(all(target_arch = "wasm32", feature = "browser"))]
mod browser_canvas;
#[cfg(all(target_arch = "wasm32", feature = "browser"))]
pub mod browser_game;
#[cfg(test)]
mod browser_hid_fixtures;
/// Portable bounded HID profile setup using the common platform decoder.
pub mod browser_hid_input;
#[cfg(test)]
mod browser_hid_runtime_fixtures;
/// Portable bounded preparation and decoding of canonical browser physical input.
pub mod browser_input;
#[cfg(test)]
mod browser_input_fixtures;
/// Worker bindings for the common nonblocking local gameplay owner.
#[cfg(all(target_arch = "wasm32", feature = "browser"))]
pub mod browser_local_game;
#[cfg(test)]
mod browser_local_input_fixtures;
#[cfg(test)]
mod browser_local_saved_fixtures;
/// Browser bindings for the common multiplayer session and bounded framing.
#[cfg(all(target_arch = "wasm32", feature = "browser"))]
pub mod browser_multiplayer;
#[cfg(all(target_arch = "wasm32", feature = "browser"))]
pub mod browser_replay;
#[cfg(test)]
mod browser_touch_fixtures;
/// Strict bounded application chart decoding before the UTF-8 parser.
pub mod chart_text;
/// Saved-record opponents and actual judgment summaries.
pub mod competition;
/// Application competition options and native runtime observation.
pub mod competition_live;
/// Actual judge completion and native output drain for full-song play.
pub mod completion;
#[cfg(test)]
mod contact_input_mode_fixtures;
/// Portable bounded audio device metadata and explicit draft selection.
pub mod device_catalog;
#[cfg(test)]
mod finite_replay_fixtures;
#[cfg(test)]
mod finite_step_gameplay_fixtures;
#[cfg(test)]
mod finite_step_replay_fixtures;
/// Native FLAC asset decoding during bounded preparation.
pub mod flac_decode;
#[cfg(test)]
mod flac_fixture;
/// Original bitmap glyph atlas data, prepared outside rendering callbacks.
#[cfg(feature = "graphics")]
pub mod font;
/// Bounded portable font rasterization and immutable glyph atlas placement.
#[cfg(feature = "graphics")]
pub mod font_atlas;
#[cfg(all(feature = "graphics", test))]
mod font_atlas_fixtures;
#[cfg(all(feature = "graphics", test))]
mod font_fixture;
/// Cached font glyphs composed through the existing ordered sprite path.
#[cfg(feature = "graphics")]
pub mod font_text;
/// Reusable asynchronous native/Web GPU presentation, separate from game I/O.
#[cfg(feature = "graphics")]
pub mod graphics;
/// Contained immutable visual assets prepared outside playback callbacks.
pub mod image_assets;
/// Fixed transparent crop canvases prepared from original image resources.
pub mod image_crop;
/// Bounded raster decoding during preparation, independent of GPU ownership.
pub mod image_decode;
/// Fixed-capacity lane feedback from actual local judge results and song time.
pub mod judge_feedback;
pub mod live_pause;
/// Bounded native input merging on one common host clock.
pub mod local_input;
/// Collection-based local player identity and unique native input assignment.
pub mod local_players;
/// Host-independent prepared judges, bindings and disjoint key-sound voices.
pub mod local_preparation;
#[cfg(test)]
mod local_preparation_fixtures;
/// Shared-transport/output execution over independent actual core runtimes.
pub mod local_runtime;
/// Graphical local-player draft using typed keyboard metadata and stable IDs.
pub mod local_setup;
#[cfg(test)]
mod local_source_plan_fixtures;
/// Complete MPEG Layer III assets and declared encoder timing during preparation.
pub mod mp3_decode;
#[cfg(test)]
mod mp3_fixture;
/// Bounded two-player progress exchange on a dedicated socket worker.
pub mod multiplayer;
/// Checked software peer-clock offset intervals and deadline conversion.
pub mod multiplayer_clock;
#[cfg(test)]
mod multiplayer_identity_fixtures;
/// Shared framed multiplayer data and state, independent of transport I/O.
pub mod multiplayer_protocol;
#[cfg(test)]
mod multiplayer_protocol_fixtures;
#[cfg(test)]
mod multiplayer_room_fixtures;
/// Bounded waiting and paired stream ownership, independent of transport I/O.
pub mod multiplayer_rooms;
#[cfg(test)]
mod multiplayer_session_fixtures;
/// Checked bilateral commitment to a future software start.
pub mod multiplayer_start;
/// Actual optional HTTP/3 WebTransport relay with bounded stream ownership.
#[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
pub mod multiplayer_webtransport;
/// Native relay client metadata and optional HTTP/3 WebTransport ownership.
pub mod multiplayer_webtransport_client;
#[cfg(test)]
mod multiplayer_webtransport_client_fixtures;
#[cfg(all(test, not(target_arch = "wasm32"), feature = "webtransport"))]
mod multiplayer_webtransport_fixtures;
/// Common queue, rolling BGM and mixer construction/replenishment.
pub mod native_audio;
/// Shared native chart loading, section slicing and retained-lane coverage.
pub mod native_chart;
/// Shared native local-cohort gameplay and per-player report ownership.
pub mod native_cohort;
/// Shared local member construction, activation and recording finalization.
pub mod native_cohort_setup;
/// Omitted solo option defaults, independent of native discovery.
pub mod native_defaults;
pub mod native_end;
/// Common post-cleanup solo finalization and exclusive capture publication.
pub mod native_finish;
/// Shared native solo gameplay sequencing behind device operations.
pub mod native_gameplay;
/// Shared native profile, completion and optional capture configuration.
pub mod native_judge;
/// Checked nominal session/host/output projection for future native frame startup.
pub mod native_start;
/// Full-prefix prepared-object presentation state.
pub mod note_progress;
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
/// Shared playfield partitions for rendering and projected physical touch routing.
pub mod playfield_layout;
/// Exact original-song practice positions and native-setting draft updates.
pub mod practice;
pub mod practice_loop;
/// Portable display configuration shared by CLI, graphical drafts and profiles.
pub mod presentation_settings;
#[cfg(test)]
mod pressed_contact_fixtures;
/// Ownership of actual admitted BMS lane buttons.
pub mod pressed_keys;
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
/// Retained bounded summaries for the common competition scoreboard.
pub mod saved_opponent_hud;
#[cfg(test)]
mod saved_opponent_hud_fixtures;
/// Bounded canonical saved-record opponents using the actual comparison engine.
pub mod saved_opponents;
#[cfg(test)]
mod saved_opponents_fixtures;
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
#[cfg(test)]
mod source_preparation_fixtures;
/// Nonblocking solo/local runtimes with shared bounded outgoing audio ownership.
pub mod step_gameplay;
#[cfg(test)]
mod step_gameplay_fixtures;
#[cfg(test)]
mod step_local_gameplay_fixtures;
/// Nonblocking recorded-operation presentation and bounded remote audio batches.
pub mod step_replay;
#[cfg(test)]
mod step_replay_fixtures;
/// Validated portable raw texture resources.
pub mod texture;
#[cfg(test)]
mod touch_gameplay_fixtures;
/// Atomic Design-style presentation compositions, independent of native I/O.
#[cfg(feature = "graphics")]
pub mod ui;
/// Complete single-stream Ogg/Vorbis assets decoded during preparation.
pub mod vorbis_decode;
#[cfg(test)]
mod vorbis_fixture;

use beatkernel::replay::codec::{ReplayCodecLimits, ReplayFile, encode_replay};
use beatkernel::{
    audio::{AudioCommand, AudioFormat, PcmLimits, PcmSample, SampleBank, SampleId, VoiceId},
    judge::JudgeStage,
    runtime::SoundBinding,
};
use beatkernel_bms::{BmsChart, CompiledBms, ParseOptions, parse_seeded};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    path::{Path, PathBuf},
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

/// Explicit strict RIFF WAVE decoder; no additional formats are implied.
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

/// Default off-thread asset decoding by FLAC/Ogg/MPEG signatures or strict WAV.
/// Other formats reject; neither extension replacement nor resampling occurs here.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultAssetDecoder;
impl AssetDecoder for DefaultAssetDecoder {
    fn decode(
        &self,
        path: &Path,
        encoded: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>> {
        if encoded.starts_with(b"fLaC") {
            flac_decode::FlacDecoder.decode(path, encoded, limits)
        } else if encoded.starts_with(b"OggS") {
            vorbis_decode::VorbisDecoder.decode(path, encoded, limits)
        } else if encoded.starts_with(b"ID3")
            || encoded
                .get(..2)
                .is_some_and(|header| header[0] == 0xff && header[1] & 0xe0 == 0xe0)
        {
            mp3_decode::Mp3Decoder.decode(path, encoded, limits)
        } else {
            WavDecoder.decode(path, encoded, limits)
        }
    }
}

/// Prepare a bounded UTF-8 or Shift-JIS BMS and WAV/FLAC/Vorbis/MP3 assets off-thread.
pub fn load_prepared(
    path: &Path,
    format: AudioFormat,
    pcm_limits: PcmLimits,
    channels: ChannelPolicy,
) -> Result<PreparedBms, Box<dyn Error>> {
    load_prepared_with_seed(path, format, pcm_limits, channels, 0)
}

/// Resolve a caller-selected chart seed and prepare only its selected assets.
/// This uses the default decoder and compatible path policy. The resolved
/// source and explicit seed must be supplied to capture; replay-aware loading
/// restores the stored seed before validating the setup.
pub fn load_prepared_with_seed(
    path: &Path,
    format: AudioFormat,
    pcm_limits: PcmLimits,
    channels: ChannelPolicy,
    seed: u64,
) -> Result<PreparedBms, Box<dyn Error>> {
    prepare_seeded(
        path,
        format,
        pcm_limits,
        channels,
        &DefaultAssetDecoder,
        asset_paths::AssetPathPolicy::AudioVariants,
        seed,
        None,
    )
}

/// Restore the recorded BMS branch and validate its setup before asset IO.
/// Assets use the default decoder and compatible file policy; original-song
/// section selection remains the caller's existing prepare_replay/render path.
pub fn load_prepared_for_replay(
    path: &Path,
    format: AudioFormat,
    pcm_limits: PcmLimits,
    channels: ChannelPolicy,
    file: &ReplayFile,
    limits: ReplayCodecLimits,
) -> Result<PreparedBms, Box<dyn Error>> {
    encode_replay(file, limits)?;
    let (_, _, seed) = replay_playback::decode_chart_setup(&file.header.options)?;
    prepare_seeded(
        path,
        format,
        pcm_limits,
        channels,
        &DefaultAssetDecoder,
        asset_paths::AssetPathPolicy::AudioVariants,
        seed,
        Some((file, limits)),
    )
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
    load_prepared_with_decoder_and_paths(
        path,
        format,
        pcm_limits,
        channels,
        decoder,
        asset_paths::AssetPathPolicy::Exact,
    )
}

/// Prepare with an explicit codec and exact/compatible contained file policy.
/// Original BMS references and chart identity stay unchanged; the decoder sees
/// the selected canonical regular file. Existing-path errors never fall through.
pub fn load_prepared_with_decoder_and_paths(
    path: &Path,
    format: AudioFormat,
    pcm_limits: PcmLimits,
    channels: ChannelPolicy,
    decoder: &dyn AssetDecoder,
    paths: asset_paths::AssetPathPolicy,
) -> Result<PreparedBms, Box<dyn Error>> {
    prepare_seeded(path, format, pcm_limits, channels, decoder, paths, 0, None)
}

fn prepare_seeded(
    path: &Path,
    format: AudioFormat,
    pcm_limits: PcmLimits,
    channels: ChannelPolicy,
    decoder: &dyn AssetDecoder,
    paths: asset_paths::AssetPathPolicy,
    seed: u64,
    replay: Option<(&ReplayFile, ReplayCodecLimits)>,
) -> Result<PreparedBms, Box<dyn Error>> {
    let chart_path = std::fs::canonicalize(path)?;
    let root = chart_path.parent().ok_or("chart has no parent")?;
    let encoded_chart = bounded_read(&chart_path, ParseOptions::default().max_bytes)?;
    let source = asset_source::FileAssetSource::new(root)?;
    prepare_from_source(
        &encoded_chart,
        &source,
        format,
        pcm_limits,
        channels,
        decoder,
        paths,
        seed,
        replay,
    )
}

/// Prepares the same chart and assets from bounded bytes and a scoped resource source.
/// Replay setup is validated before any resource acquisition.
/// Equal resolved keys reuse decoding within this call; source bytes and the
/// decoder must remain stable during preparation. Each sample ID owns its PCM.
pub fn prepare_from_source(
    chart_bytes: &[u8],
    assets: &dyn asset_source::AssetSource,
    format: AudioFormat,
    pcm_limits: PcmLimits,
    channels: ChannelPolicy,
    decoder: &dyn AssetDecoder,
    paths: asset_paths::AssetPathPolicy,
    seed: u64,
    replay: Option<(&ReplayFile, ReplayCodecLimits)>,
) -> Result<PreparedBms, Box<dyn Error>> {
    let options = ParseOptions::default();
    let text = chart_text::decode_chart_text(
        chart_bytes,
        chart_text::ChartTextEncoding::Auto,
        options.max_bytes,
    )?;
    let source = parse_seeded(&text, options, seed)?;
    let wav_gain = source.wav_gain()?;
    if let Some((file, limits)) = replay {
        replay_playback::validate_setup(&source, file, limits)?;
    }
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
    let mut decoded: BTreeMap<PathBuf, SampleId> = BTreeMap::new();
    for sample in referenced {
        let name = source
            .samples
            .get(&u16::try_from(sample.0)?)
            .ok_or("referenced sample has no WAV definition")?;
        let asset_path = assets.resolve(name, paths)?;
        if let Some(&first) = decoded.get(&asset_path) {
            let pcm = bank
                .get(first)
                .expect("cached PCM was inserted into this bank")
                .try_clone(pcm_limits)?;
            bank.insert(sample, pcm)?;
            continue;
        }
        let encoded = assets.read(&asset_path, 64 * 1024 * 1024)?;
        if encoded.len() > 64 * 1024 * 1024 {
            return Err("encoded file exceeds preparation limit".into());
        }
        let pcm = decoder.decode(&asset_path, &encoded, pcm_limits)?;
        let pcm = prepare_channels(pcm, format, pcm_limits, channels)?;
        bank.insert(sample, pcm)?;
        decoded.insert(asset_path, sample);
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
            gain: wav_gain,
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
                gain: wav_gain,
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
    Ok(asset_source::read_bounded(path, limit)?)
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

/// Exact accepted builtin-stage timing summaries.
pub mod timing;
/// Integer-only timing presentation labels.
pub mod timing_display;

/// UI-owned bounded GPU background cache and ordered static image composition.
#[cfg(feature = "graphics")]
pub mod bga_render;

/// Immutable exact-black transparency for chart-declared BGA Layer resources.
pub mod image_key;

/// Original-song-time activation policy for retained Poor image selections.
pub mod poor_background;

/// Shared native QUIC ownership and portable explicit credential configuration.
pub mod multiplayer_quic;
#[cfg(test)]
mod multiplayer_quic_fixtures;

/// Portable ownership of the actual Mixer on an absolute callback frame grid.
pub mod worklet_audio;
#[cfg(test)]
mod worklet_audio_fixtures;

/// Prepared original-song per-role BGA opacity queries.
pub mod bga_opacity;
