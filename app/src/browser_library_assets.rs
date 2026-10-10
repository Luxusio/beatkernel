//! Pure selected-file dependency planning; no media acquisition or decoding.
use crate::{
    asset_paths::AssetPathPolicy,
    asset_source::{AssetSource, MemoryFiles},
    image_assets::{ImageAssetLimits, ImageAssets},
    video_assets::{VideoAssetLimits, VideoAssets},
};
use beatkernel::{audio::SampleId, replay::codec::decode_replay};
use beatkernel_bms::{BmsChart, ParseOptions};
use std::{collections::BTreeSet, error::Error};

fn chart(files: &MemoryFiles, path: &str, seed: u64) -> Result<BmsChart, Box<dyn Error>> {
    let options = ParseOptions::default();
    let bytes = files.read_file(path, options.max_bytes)?;
    let text = crate::chart_text::decode_chart_text(
        bytes,
        crate::chart_text::ChartTextEncoding::Auto,
        options.max_bytes,
    )?;
    Ok(beatkernel_bms::parse_seeded(&text, options, seed)?)
}
fn plan(
    files: &MemoryFiles,
    path: &str,
    chart: &BmsChart,
    max_samples: usize,
) -> Result<Vec<String>, Box<dyn Error>> {
    if max_samples == 0 {
        return Err("sample capacity must be positive".into());
    }
    // Match preparation's pre-acquisition validation and references, including
    // invisible keysounds and nonfatal mines using WAV00.
    chart.wav_gain()?;
    let invisible = if chart.invisible.is_empty() {
        Vec::new()
    } else {
        chart.compile_invisible()?
    };
    let compiled = chart.compile()?;
    let referenced =
        crate::audio_assets::referenced_samples(chart, &compiled, &invisible, max_samples)?;
    let source = files.scope(path)?;
    let mut paths = BTreeSet::new();
    for SampleId(sample) in referenced {
        let name = chart
            .samples
            .get(&u16::try_from(sample)?)
            .ok_or("referenced sample has no WAV definition")?;
        paths.insert(source.resolve(name, AssetPathPolicy::AudioVariants)?);
    }
    paths.extend(ImageAssets::referenced_paths(
        &source,
        chart,
        ImageAssetLimits::default(),
    )?);
    paths.extend(VideoAssets::referenced_paths(
        &source,
        chart,
        VideoAssetLimits::default(),
    )?);
    paths
        .into_iter()
        .map(|path| {
            path.into_os_string()
                .into_string()
                .map_err(|_| "selected asset key is not UTF-8".into())
        })
        .collect()
}
/// Return sorted unique canonical keys, resolving unloaded declarations too.
pub fn referenced_asset_paths(
    files: &MemoryFiles,
    path: &str,
    seed: u64,
    max_samples: usize,
) -> Result<Vec<String>, Box<dyn Error>> {
    let chart = chart(files, path, seed)?;
    plan(files, path, &chart, max_samples)
}
/// The recorded setup supplies the seed; its identity is checked before any media read.
pub fn replay_referenced_asset_paths(
    files: &MemoryFiles,
    path: &str,
    replay_bytes: &[u8],
    max_samples: usize,
) -> Result<Vec<String>, Box<dyn Error>> {
    let limits = crate::competition_live::replay_limits()?;
    let file = decode_replay(replay_bytes, limits)?;
    let setup = crate::replay_playback::decode_section_setup(&file.header.options)?;
    let chart = chart(files, path, setup.chart_seed)?;
    crate::replay_playback::validate_section_setup(&chart, &file, limits)?;
    plan(files, path, &chart, max_samples)
}
