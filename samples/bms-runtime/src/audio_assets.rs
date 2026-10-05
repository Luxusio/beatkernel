//! Internal referenced-sample selection and bounded PCM bank preparation.

use crate::{
    AssetDecoder, ChannelPolicy, asset_paths::AssetPathPolicy, asset_source::AssetSource,
    mine_plan::MinePlan,
};
use beatkernel::audio::{AudioFormat, PcmLimits, SampleBank, SampleId};
use beatkernel_bms::{BmsChart, CompiledBms, ParseOptions, ScheduledInvisible};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    path::PathBuf,
};

/// Selects original resource identities before any resource acquisition.
pub(crate) fn referenced_samples(
    source: &BmsChart,
    compiled: &CompiledBms,
    invisible: &[ScheduledInvisible],
    max_samples: usize,
) -> Result<BTreeSet<SampleId>, Box<dyn Error>> {
    let mut referenced: BTreeSet<_> = source
        .notes
        .iter()
        .map(|note| note.sample)
        .chain(compiled.bgm.iter().map(|event| event.sample))
        .chain(invisible.iter().map(|event| event.sample))
        .collect();
    if !source.mines.is_empty() {
        let mines = MinePlan::prepare(source, ParseOptions::default().max_objects)?;
        if source.samples.contains_key(&0)
            && mines.markers().iter().any(|mine| !mine.damage.is_fatal())
        {
            referenced.insert(SampleId(0));
        }
    }
    if referenced.len() > max_samples {
        return Err("referenced asset count exceeds PCM limits".into());
    }
    Ok(referenced)
}

/// Loads the selected IDs with scoped lookup and call-local decode reuse.
pub(crate) fn load_bank(
    source: &BmsChart,
    referenced: &BTreeSet<SampleId>,
    assets: &dyn AssetSource,
    format: AudioFormat,
    pcm_limits: PcmLimits,
    channels: ChannelPolicy,
    decoder: &dyn AssetDecoder,
    paths: AssetPathPolicy,
) -> Result<SampleBank, Box<dyn Error>> {
    if referenced.len() > pcm_limits.max_samples() {
        return Err("referenced asset count exceeds PCM limits".into());
    }
    let mut bank = SampleBank::new(format, pcm_limits)?;
    let mut decoded: BTreeMap<PathBuf, SampleId> = BTreeMap::new();
    for &sample in referenced {
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
        let pcm = crate::prepare_channels(pcm, format, pcm_limits, channels)?;
        bank.insert(sample, pcm)?;
        decoded.insert(asset_path, sample);
    }
    Ok(bank)
}
