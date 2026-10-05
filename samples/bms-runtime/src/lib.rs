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
mod audio_assets;
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
/// Worker bindings for the common bounded room-admission client.
#[cfg(all(target_arch = "wasm32", feature = "browser"))]
pub mod browser_room_client;
#[cfg(test)]
mod browser_touch_fixtures;
#[cfg(test)]
mod catalog_read_budget_fixtures;
#[cfg(test)]
mod chart_read_budget_fixtures;
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
#[cfg(all(feature = "graphics", test))]
mod font_grapheme_window_fixtures;
/// Cached font glyphs composed through the existing ordered sprite path.
#[cfg(feature = "graphics")]
pub mod font_text;
/// Explicit competition observation and completion ports for gameplay policy.
pub mod gameplay_competition;
/// Business presentation and device contracts with injected implementations.
pub mod gameplay_presentation;
#[cfg(test)]
mod gameplay_presentation_port_fixtures;
/// Shared fixed-point gauge observations from committed normal and mine outcomes.
pub mod gauge;
#[cfg(test)]
mod gauge_fence_fixtures;
#[cfg(test)]
mod gauge_fixtures;
#[cfg(test)]
mod gauge_hud_fixtures;
#[cfg(test)]
mod gauge_sound_stop_fixtures;
/// Reusable asynchronous native/Web GPU presentation, separate from game I/O.
#[cfg(feature = "graphics")]
pub mod graphics;
/// Contained immutable visual assets prepared outside playback callbacks.
pub mod image_assets;
#[cfg(test)]
mod image_base_fixtures;
/// Fixed transparent crop canvases prepared from original image resources.
pub mod image_crop;
/// Bounded raster decoding during preparation, independent of GPU ownership.
pub mod image_decode;
#[cfg(test)]
mod image_variant_fixtures;
/// Pure BMS input-sound timing and dedicated reusable voice preparation.
pub mod input_sounds;
#[cfg(test)]
mod input_sounds_fixtures;
#[cfg(test)]
mod invisible_admission_fixtures;
#[cfg(test)]
mod invisible_audio_fixtures;
#[cfg(test)]
mod invisible_contact_fixtures;
#[cfg(test)]
mod invisible_identity_fixtures;
#[cfg(test)]
mod invisible_lane_fixtures;
#[cfg(test)]
mod invisible_source_fixtures;
/// Fixed-capacity lane feedback from actual local judge results and song time.
pub mod judge_feedback;
pub mod live_pause;
#[cfg(test)]
mod local_fence_fixtures;
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
mod local_sound_stop_fixtures;
#[cfg(test)]
mod local_source_plan_fixtures;
#[cfg(test)]
mod mine_admission_fixtures;
#[cfg(test)]
mod mine_asset_fixtures;
#[cfg(test)]
mod mine_audio_consumers_fixtures;
/// Checked committed mine damage evidence, independent of gauge and audio policy.
pub mod mine_damage;
#[cfg(test)]
mod mine_damage_fixtures;
#[cfg(test)]
mod mine_extent_fixtures;
/// Shared original mine timing and source-aware pristine judge preparation.
pub mod mine_plan;
#[cfg(test)]
mod mine_plan_fixtures;
#[cfg(test)]
mod mine_render_fixtures;
#[cfg(test)]
mod mine_sound_fixtures;
/// Optional original WAV00 bindings and source sound identity.
pub mod mine_sounds;
/// Complete MPEG Layer III assets and declared encoder timing during preparation.
pub mod mp3_decode;
#[cfg(test)]
mod mp3_fixture;
/// Bounded two-player progress exchange on a dedicated socket worker.
pub mod multiplayer;
/// Checked software peer-clock offset intervals and deadline conversion.
pub mod multiplayer_clock;
/// Bounded exact whole-cohort progress payloads, independent of stream ownership.
pub mod multiplayer_group;
#[cfg(test)]
mod multiplayer_group_fixtures;
/// Bounded collecting and prepared host rosters, independent of transport I/O.
pub mod multiplayer_group_rooms;
#[cfg(test)]
mod multiplayer_group_session_fixtures;
#[cfg(test)]
mod multiplayer_identity_fixtures;
/// Shared framed multiplayer data and state, independent of transport I/O.
pub mod multiplayer_protocol;
#[cfg(test)]
mod multiplayer_protocol_fixtures;
/// Common room admission state and bounded explicit Read/Write driving.
pub mod multiplayer_room_client;
/// Shared prepared-room probes and complete-write clock barriers.
pub mod multiplayer_room_clock;
#[cfg(test)]
mod multiplayer_room_clock_fixtures;
#[cfg(test)]
mod multiplayer_room_fixtures;
/// Timed incremental stream driving for common room admission and software start.
pub mod multiplayer_room_io;
#[cfg(test)]
mod multiplayer_room_io_fixtures;
/// Common client composition from room admission through committed start.
pub mod multiplayer_room_play;
#[cfg(test)]
mod multiplayer_room_play_fixtures;
/// Participant-scoped coalesced room progress and actual final recipient ACKs.
pub mod multiplayer_room_progress;
/// Common participant publication, peer receipt and local final ACK ownership.
pub mod multiplayer_room_progress_client;
#[cfg(test)]
mod multiplayer_room_progress_client_fixtures;
#[cfg(test)]
mod multiplayer_room_progress_fixtures;
/// Shared prepared multi-host software-start coordination.
pub mod multiplayer_room_start;
#[cfg(test)]
mod multiplayer_room_start_fixtures;
/// Distinct bounded BKMR room admission messages and incremental framing.
pub mod multiplayer_room_wire;
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
/// Owned native background scanning and CPU catalog preparation.
#[cfg(not(target_arch = "wasm32"))]
pub mod native_catalog;
/// Shared native chart loading, section slicing and retained-lane coverage.
pub mod native_chart;
/// Shared native local-cohort gameplay and per-player report ownership.
pub mod native_cohort;
/// Shared local member construction, activation and recording finalization.
pub mod native_cohort_setup;
/// One application routing boundary for bilateral and multi-host room competition.
pub mod native_competition_network;
/// Omitted solo option defaults, independent of native discovery.
pub mod native_defaults;
pub mod native_end;
/// Common post-cleanup solo finalization and exclusive capture publication.
pub mod native_finish;
/// Shared native solo gameplay sequencing behind device operations.
pub mod native_gameplay;
mod native_gameplay_bridge;
/// Explicit command, publication and diagnostic port for native gameplay policy.
pub mod native_gameplay_host;
#[cfg(test)]
mod native_gauge_fixtures;
/// One shared network/start owner over actual native local-member prefixes.
pub mod native_group_competition;
#[cfg(test)]
mod native_invisible_audio_fixtures;
#[cfg(test)]
mod native_invisible_identity_fixtures;
/// Shared native profile, completion and optional capture configuration.
pub mod native_judge;
#[cfg(test)]
mod native_local_presentation_port_fixtures;
#[cfg(test)]
mod native_mine_audio_fixtures;
#[cfg(test)]
mod native_mine_fixtures;
#[cfg(test)]
mod native_mine_presentation_fixtures;
/// Injectable diagnostic deadlines and waiting for shared native pumps.
pub mod native_pump_control;
mod native_pump_system;
/// Game-owned room lobby, comparison, committed start and natural finalization.
#[cfg(not(target_arch = "wasm32"))]
pub mod native_room_competition;
/// Dedicated native room network thread, bounded commands and retained real receipts.
#[cfg(not(target_arch = "wasm32"))]
pub mod native_room_network;
#[cfg(test)]
mod native_solo_presentation_port_fixtures;
/// Checked nominal session/host/output projection for future native frame startup.
pub mod native_start;
/// Full-prefix prepared-object presentation state.
pub mod note_progress;
/// Synthetic offline composition using the same runtime and mixer as native apps.
pub mod offline;
#[cfg(test)]
mod offline_gauge_sound_stop_fixtures;
/// Typed UI panel ownership and cancellation permits for off-thread work.
pub mod panel_scope;
/// Read-only outcomes retained after actual live gameplay and output completion.
pub mod play_result;
#[cfg(test)]
mod play_result_fixtures;
/// Native presentation-derived pause and bounded keyboard reconciliation.
pub mod playback_pause;
/// Actual game-to-UI presentation and cancellation outside audio callbacks.
pub mod player;
/// Bounded chart catalog and exact compiled lane display data.
pub mod player_chart;
#[cfg(test)]
mod player_chart_scan_fixtures;
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
#[cfg(test)]
mod replay_feeder_stop_evidence_fixtures;
#[cfg(test)]
mod replay_gauge_sound_stop_fixtures;
pub mod replay_pause;
/// Checked durable replay reconstruction through the same builtin BMS judge.
pub mod replay_playback;
/// Bounded PCM rendering of captured BMS play through the actual core Mixer.
pub mod replay_render;
#[cfg(test)]
mod replay_render_owned_stop_fixtures;
/// Incremental presentation of validated recorded judging operations.
pub mod replay_visual;
/// Retained participant-scoped room scores and bounded borrowed pages.
pub mod room_opponent_hud;
#[cfg(test)]
mod room_opponent_hud_fixtures;
/// Portable room lobby actions, cached score pages and immutable joined Results.
pub mod room_presentation;
/// Portable bounded reconstruction of immutable joined room Results.
pub mod room_results_builder;
#[cfg(test)]
mod room_results_builder_fixtures;
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
mod step_local_failed_terminal_fixtures;
#[cfg(test)]
mod step_local_gameplay_fixtures;
#[cfg(test)]
mod step_local_play_result_fixtures;
#[cfg(test)]
mod step_local_stop_ack_fixtures;
/// Nonblocking recorded-operation presentation and bounded remote audio batches.
pub mod step_replay;
#[cfg(test)]
mod step_replay_fixtures;
#[cfg(test)]
mod step_replay_stop_ack_fixtures;
#[cfg(test)]
mod step_solo_failed_terminal_fixtures;
#[cfg(test)]
mod step_solo_play_result_fixtures;
#[cfg(test)]
mod step_solo_stop_ack_fixtures;
/// Validated portable raw texture resources.
pub mod texture;
#[cfg(test)]
mod touch_gameplay_fixtures;
#[cfg(test)]
mod touch_page_fixtures;
/// Atomic Design-style presentation compositions, independent of native I/O.
#[cfg(feature = "graphics")]
pub mod ui;
pub mod viewport;
#[cfg(test)]
mod viewport_fixtures;
#[cfg(test)]
mod viewport_touch_fixtures;
/// Complete single-stream Ogg/Vorbis assets decoded during preparation.
pub mod vorbis_decode;
#[cfg(test)]
mod vorbis_fixture;

use beatkernel::replay::codec::{ReplayCodecLimits, ReplayFile, encode_replay};
use beatkernel::{
    audio::{AudioCommand, AudioFormat, PcmLimits, PcmSample, SampleBank, VoiceId},
    judge::JudgeStage,
    runtime::SoundBinding,
};
use beatkernel_bms::{BmsChart, CompiledBms, ParseOptions, parse_seeded};
use std::{collections::BTreeMap, error::Error, path::Path};

/// Default original PCM sample capacity for the complete two-digit base62 namespace.
pub const DEFAULT_BMS_PCM_SAMPLES: usize = 62 * 62;

#[cfg(test)]
mod pcm_radix_fixtures;

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
/// Unsupported mine gameplay is refused immediately after parsing, before gain,
/// replay validation or resource acquisition.
/// WAV gain and invisible timing are validated before replay setup; replay setup
/// is then validated before acquiring the unique visible/BGM/invisible resources.
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
    if !source.mines.is_empty() {
        return Err("mine gameplay is not supported during preparation".into());
    }
    let wav_gain = source.wav_gain()?;
    let invisible = if source.invisible.is_empty() {
        Vec::new()
    } else {
        source.compile_invisible()?
    };
    if let Some((file, limits)) = replay {
        replay_playback::validate_setup(&source, file, limits)?;
    }
    let compiled = source.compile()?;
    let referenced =
        audio_assets::referenced_samples(&source, &compiled, &invisible, pcm_limits.max_samples())?;
    let bank = audio_assets::load_bank(
        &source,
        &referenced,
        assets,
        format,
        pcm_limits,
        channels,
        decoder,
        paths,
    )?;

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
