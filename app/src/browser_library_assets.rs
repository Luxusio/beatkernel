//! Canonical selected-file planning and owned browser preparation; no async borrows.
use crate::{
    asset_paths::AssetPathPolicy,
    asset_source::{AssetSource, MemoryFiles},
    image_assets::{ImageAssetLimits, ImageAssets, ImagePreparationJob},
    preparation::{
        PreparationAdmission, PreparationAdvance, PreparationCache, PreparationCacheLimits,
        PreparationCandidate, PreparationCounters, PreparationJob, PreparationPlan,
        PreparationProgress, PreparationRequest, PreparationStage,
    },
    video_assets::{VideoAssetLimits, VideoAssets, VideoPreparationJob, VideoPreparationMode},
    ChannelPolicy, PreparedBms,
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits},
    replay::codec::{decode_replay, ReplayFile},
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    error::Error,
    path::Path,
    sync::Arc,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn request(path: &str, format: AudioFormat, seed: u64, limits: PcmLimits) -> PreparationRequest {
    PreparationRequest {
        source_identity: path.to_owned(),
        seed,
        format,
        limits,
        channels: ChannelPolicy::MonoToStereo,
        paths: AssetPathPolicy::AudioVariants,
    }
}
fn chart_bytes(files: &MemoryFiles, path: &str) -> Result<Vec<u8>> {
    let bytes = files.read_file(path, beatkernel_bms::ParseOptions::default().max_bytes)?;
    let mut owned = Vec::new();
    owned.try_reserve_exact(bytes.len())?;
    owned.extend_from_slice(bytes);
    Ok(owned)
}
fn reference_plan(
    files: &MemoryFiles,
    path: &str,
    seed: u64,
    max_samples: usize,
    replay: Option<&ReplayFile>,
) -> Result<Vec<String>> {
    let limits = PcmLimits::new(64 * 1024 * 1024, 256 * 1024 * 1024, max_samples)?;
    let mut job = PreparationJob::uncached(
        request(path, AudioFormat::new(48000, 2)?, seed, limits),
        chart_bytes(files, path)?,
    )?;
    let source = files.scope(path)?;
    let replay_limits = crate::competition_live::replay_limits()?;
    while job.parsed().is_none() {
        job.advance(
            &source,
            replay.map(|file| (file, replay_limits)),
            &mut |_| Ok(()),
        )?;
    }
    let parsed = job.parsed().ok_or("parsed dependency plan unavailable")?;
    collect_paths(parsed, &source)
}
fn collect_paths(
    parsed: &crate::preparation::ParsedPreparation,
    source: &dyn AssetSource,
) -> Result<Vec<String>> {
    let mut paths = BTreeSet::new();
    for id in parsed.referenced_samples() {
        let name = parsed
            .source()
            .samples
            .get(&u16::try_from(id.0)?)
            .ok_or("referenced sample has no WAV definition")?;
        paths.insert(source.resolve(name, AssetPathPolicy::AudioVariants)?);
    }
    paths.extend(ImageAssets::referenced_paths(
        source,
        parsed.source(),
        ImageAssetLimits::default(),
    )?);
    paths.extend(VideoAssets::referenced_paths(
        source,
        parsed.source(),
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
/// Compatibility dependency query uses the same original parser/compiler as jobs.
pub fn referenced_asset_paths(
    files: &MemoryFiles,
    path: &str,
    seed: u64,
    max_samples: usize,
) -> Result<Vec<String>> {
    reference_plan(files, path, seed, max_samples, None)
}
pub fn replay_referenced_asset_paths(
    files: &MemoryFiles,
    path: &str,
    replay_bytes: &[u8],
    max_samples: usize,
) -> Result<Vec<String>> {
    let file = decode_replay(replay_bytes, crate::competition_live::replay_limits()?)?;
    let setup = crate::replay_playback::decode_section_setup(&file.header.options)?;
    reference_plan(files, path, setup.chart_seed, max_samples, Some(&file))
}

/// One library's last immutable original; declarations and hydration remain in MemoryFiles.
pub struct BrowserPreparationCache {
    cache: RefCell<PreparationCache>,
    identity: Arc<()>,
    inventory: RefCell<Option<Arc<()>>>,
    revision: Cell<Option<u64>>,
    plan: RefCell<Option<PreparationPlan>>,
    query_counters: Cell<PreparationCounters>,
}
impl BrowserPreparationCache {
    pub fn new() -> Result<Self> {
        Ok(Self {
            cache: RefCell::new(PreparationCache::new(PreparationCacheLimits::default())?),
            identity: Arc::new(()),
            inventory: RefCell::new(None),
            revision: Cell::new(None),
            plan: RefCell::new(None),
            query_counters: Cell::new(PreparationCounters::default()),
        })
    }
    pub fn begin(
        &self,
        files: &MemoryFiles,
        path: &str,
        format: AudioFormat,
        seed: u64,
        limits: PcmLimits,
        replay: Option<ReplayFile>,
    ) -> Result<BrowserAssetPreparationJob> {
        if let Some(file) = &replay {
            let setup = crate::replay_playback::decode_section_setup(&file.header.options)?;
            if setup.chart_seed != seed {
                return Err("replay seed differs from recorded setup".into());
            }
        }
        let bytes = chart_bytes(files, path)?;
        let revision = self.synchronize(files);
        let request = request(path, format, seed, limits);
        let audio = if let Some(plan) = self.plan.borrow().as_ref() {
            self.cache.borrow().begin_with_plan(request, bytes, plan)?
        } else {
            self.cache.borrow().begin(request, bytes)?
        };
        Ok(BrowserAssetPreparationJob {
            identity: Arc::clone(&self.identity),
            inventory: Arc::clone(files.inventory_identity()),
            revision,
            path: path.to_owned(),
            audio,
            candidate: None,
            image_job: None,
            video_job: None,
            images: None,
            movies: None,
            replay,
            phase: AssetPhase::Audio,
        })
    }
    fn synchronize(&self, files: &MemoryFiles) -> u64 {
        let revision = files.inventory_revision();
        if self.revision.get().is_some_and(|old| old != revision)
            || self
                .inventory
                .borrow()
                .as_ref()
                .is_some_and(|old| !Arc::ptr_eq(old, files.inventory_identity()))
        {
            self.cache.borrow_mut().clear();
            self.plan.borrow_mut().take();
        }
        *self.inventory.borrow_mut() = Some(Arc::clone(files.inventory_identity()));
        self.revision.set(Some(revision));
        revision
    }
    pub fn query_counters(&self) -> PreparationCounters {
        self.query_counters.get()
    }
    pub fn referenced_asset_paths(
        &self,
        files: &MemoryFiles,
        path: &str,
        seed: u64,
        max_samples: usize,
    ) -> Result<Vec<String>> {
        self.query(files, path, seed, max_samples, None)
    }
    pub fn replay_referenced_asset_paths(
        &self,
        files: &MemoryFiles,
        path: &str,
        bytes: &[u8],
        max_samples: usize,
    ) -> Result<Vec<String>> {
        let file = decode_replay(bytes, crate::competition_live::replay_limits()?)?;
        let setup = crate::replay_playback::decode_section_setup(&file.header.options)?;
        self.query(files, path, setup.chart_seed, max_samples, Some(&file))
    }
    fn query(
        &self,
        files: &MemoryFiles,
        path: &str,
        seed: u64,
        max_samples: usize,
        replay: Option<&ReplayFile>,
    ) -> Result<Vec<String>> {
        let limits = PcmLimits::new(64 * 1024 * 1024, 256 * 1024 * 1024, max_samples)?;
        let mut job = self.begin(files, path, AudioFormat::new(48000, 2)?, seed, limits, None)?;
        let source = files.scope(path)?;
        let replay_limits = crate::competition_live::replay_limits()?;
        let outcome = (|| {
            while job.audio.parsed().is_none() {
                job.audio.advance(
                    &source,
                    replay.map(|file| (file, replay_limits)),
                    &mut |_| Ok(()),
                )?;
            }
            let paths = collect_paths(
                job.audio
                    .parsed()
                    .ok_or("parsed dependency plan unavailable")?,
                &source,
            )?;
            *self.plan.borrow_mut() = job.audio.shared_plan();
            Ok(paths)
        })();
        let counts = job.counters();
        let mut total = self.query_counters.get();
        total.parses = total
            .parses
            .checked_add(counts.parses)
            .ok_or("query parse counter exhausted")?;
        total.compiles = total
            .compiles
            .checked_add(counts.compiles)
            .ok_or("query compile counter exhausted")?;
        self.query_counters.set(total);
        outcome
    }
    pub fn retained_bytes(&self) -> crate::preparation::PreparationRetainedBytes {
        self.cache.borrow().retained_bytes()
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum AssetPhase {
    Audio,
    Images,
    Movies,
    Ready,
    Staged,
    Cancelled,
    Taken,
}
/// Fresh images and movie descriptors accompany shared immutable original PCM.
pub struct BrowserAssetPrepared {
    pub prepared: PreparedBms,
    pub images: ImageAssets,
    pub movies: VideoAssets,
    pub replay: Option<ReplayFile>,
}
pub struct BrowserAssetPreparationJob {
    identity: Arc<()>,
    inventory: Arc<()>,
    revision: u64,
    path: String,
    audio: PreparationJob,
    candidate: Option<PreparationCandidate>,
    image_job: Option<ImagePreparationJob>,
    video_job: Option<VideoPreparationJob>,
    images: Option<ImageAssets>,
    movies: Option<VideoAssets>,
    replay: Option<ReplayFile>,
    phase: AssetPhase,
}
impl BrowserAssetPreparationJob {
    pub fn validate_current(
        &self,
        cache: &BrowserPreparationCache,
        files: &MemoryFiles,
    ) -> Result<()> {
        if !Arc::ptr_eq(&self.identity, &cache.identity)
            || !Arc::ptr_eq(&self.inventory, files.inventory_identity())
            || cache
                .inventory
                .borrow()
                .as_ref()
                .is_none_or(|current| !Arc::ptr_eq(&self.inventory, current))
            || self.revision != files.inventory_revision()
            || cache.revision.get() != Some(self.revision)
        {
            return Err("foreign or changed browser preparation library".into());
        }
        if matches!(self.phase, AssetPhase::Cancelled | AssetPhase::Taken) {
            return Err("browser preparation job is terminal".into());
        }
        Ok(())
    }
    pub fn phase(&self) -> &'static str {
        match self.phase {
            AssetPhase::Images => "images",
            AssetPhase::Movies => "movies",
            AssetPhase::Ready => "ready",
            AssetPhase::Cancelled => "cancelled",
            AssetPhase::Taken => "taken",
            AssetPhase::Staged => "finalize",
            AssetPhase::Audio => match self.audio.progress().stage {
                PreparationStage::Parse => "parse",
                PreparationStage::Compile => "compile",
                PreparationStage::Resolve => "resolve",
                PreparationStage::Read => "read",
                PreparationStage::Decode => "decode",
                PreparationStage::Channels => "channels",
                PreparationStage::Admit => "admit",
                PreparationStage::Visual => "visual",
                PreparationStage::Ready => "original-ready",
            },
        }
    }
    pub fn progress(&self) -> PreparationProgress {
        match self.phase {
            AssetPhase::Images => self.image_job.as_ref().unwrap().progress(),
            AssetPhase::Movies => self.video_job.as_ref().unwrap().progress(),
            AssetPhase::Ready => PreparationProgress {
                stage: PreparationStage::Ready,
                completed_assets: 0,
                total_assets: 0,
            },
            _ => self.audio.progress(),
        }
    }
    pub fn counters(&self) -> PreparationCounters {
        self.audio.counters()
    }
    pub fn pending_resource(&self) -> Option<&Path> {
        match self.phase {
            AssetPhase::Audio => self.audio.pending_resource(),
            AssetPhase::Images => self.image_job.as_ref()?.pending_resource(),
            AssetPhase::Movies => self.video_job.as_ref()?.pending_resource(),
            _ => None,
        }
    }
    pub fn advance(
        &mut self,
        cache: &BrowserPreparationCache,
        files: &MemoryFiles,
    ) -> Result<bool> {
        let result = self.step(cache, files);
        if result.is_err() {
            self.cancel();
        }
        result
    }
    fn step(&mut self, cache: &BrowserPreparationCache, files: &MemoryFiles) -> Result<bool> {
        self.validate_current(cache, files)?;
        let source = files.scope(&self.path)?;
        match self.phase {
            AssetPhase::Audio => {
                let limits = crate::competition_live::replay_limits()?;
                if let PreparationAdvance::Ready(candidate) = self.audio.advance(
                    &source,
                    self.replay.as_ref().map(|file| (file, limits)),
                    &mut |_| Ok(()),
                )? {
                    self.image_job = Some(ImagePreparationJob::new(
                        &candidate.prepared().source,
                        ImageAssetLimits::default(),
                    )?);
                    self.video_job = Some(VideoPreparationJob::new(
                        &candidate.prepared().source,
                        VideoAssetLimits::default(),
                        VideoPreparationMode::Encoded,
                    )?);
                    self.candidate = Some(candidate);
                    self.phase = AssetPhase::Images;
                }
                if let Some(plan) = self.audio.shared_plan() {
                    *cache.plan.borrow_mut() = Some(plan);
                }
            }
            AssetPhase::Images => {
                if let Some(images) = self
                    .image_job
                    .as_mut()
                    .unwrap()
                    .advance(&source, &mut |_| Ok(()))?
                {
                    self.images = Some(images);
                    self.phase = AssetPhase::Movies;
                }
            }
            AssetPhase::Movies => {
                if let Some(movies) = self
                    .video_job
                    .as_mut()
                    .unwrap()
                    .advance(&source, &mut |_| Ok(()))?
                {
                    self.movies = Some(movies);
                    self.phase = AssetPhase::Ready;
                }
            }
            AssetPhase::Ready => return Ok(true),
            AssetPhase::Staged => {
                return Err("browser assets already staged for finalization".into())
            }
            AssetPhase::Cancelled | AssetPhase::Taken => unreachable!(),
        }
        Ok(self.phase == AssetPhase::Ready)
    }
    /// Move originals out while keeping cache admission separate from fresh finalization.
    pub fn stage_ready(
        &mut self,
        cache: &BrowserPreparationCache,
        files: &MemoryFiles,
    ) -> Result<(BrowserAssetPrepared, PreparationAdmission)> {
        self.validate_current(cache, files)?;
        if self.phase != AssetPhase::Ready {
            return Err("browser preparation is not ready".into());
        }
        let candidate = self.candidate.take().ok_or("original candidate missing")?;
        let (prepared, admission) = candidate.into_prepared_and_admission();
        self.phase = AssetPhase::Staged;
        Ok((
            BrowserAssetPrepared {
                prepared,
                images: self.images.take().unwrap(),
                movies: self.movies.take().unwrap(),
                replay: self.replay.take(),
            },
            admission,
        ))
    }
    pub fn admit(
        &mut self,
        cache: &BrowserPreparationCache,
        files: &MemoryFiles,
        admission: &PreparationAdmission,
    ) -> Result<()> {
        self.validate_current(cache, files)?;
        if self.phase != AssetPhase::Staged {
            return Err("browser preparation admission is unavailable".into());
        }
        cache.cache.borrow_mut().commit_admission(admission)?;
        self.phase = AssetPhase::Taken;
        Ok(())
    }
    /// Admit original audio plus fresh visual assets for portable asset-level callers.
    /// This does not perform section/policy/PlayerChart finalization and is not
    /// full BrowserPrepared readiness. The WASM path uses stage_ready, completes
    /// all derived validation, then calls admit with its current library.
    /// Ready then cancel never replaces the last cache.
    pub fn take_ready(
        &mut self,
        cache: &BrowserPreparationCache,
        files: &MemoryFiles,
    ) -> Result<BrowserAssetPrepared> {
        let (prepared, admission) = self.stage_ready(cache, files)?;
        self.admit(cache, files, &admission)?;
        Ok(prepared)
    }
    pub fn cancel(&mut self) {
        self.audio.cancel();
        if let Some(job) = &mut self.image_job {
            job.cancel();
        }
        if let Some(job) = &mut self.video_job {
            job.cancel();
        }
        self.candidate = None;
        self.images = None;
        self.movies = None;
        self.replay = None;
        self.phase = AssetPhase::Cancelled;
    }
}
