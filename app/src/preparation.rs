//! Cooperative original-chart preparation. Caches contain no live gameplay state.
use crate::{
    asset_paths::AssetPathPolicy, asset_source::AssetSource, audio_assets::referenced_samples,
    AssetDecoder, ChannelPolicy, DefaultAssetDecoder, PreparedBms,
};
use beatkernel::{
    audio::{AudioCommand, AudioFormat, PcmLimits, PcmSample, SampleBank, SampleId, VoiceId},
    judge::JudgeStage,
    replay::codec::{ReplayCodecLimits, ReplayFile},
    runtime::SoundBinding,
};
use beatkernel_bms::{BmsChart, CompiledBms, ParseOptions};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    error::Error,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreparationStage {
    Parse,
    Compile,
    Resolve,
    Read,
    Decode,
    Channels,
    Admit,
    Visual,
    Ready,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparationProgress {
    pub stage: PreparationStage,
    pub completed_assets: usize,
    pub total_assets: usize,
}
/// Successful original parsing/compilation, bounded encoded reads and codec
/// calls. Replay identity validation and invisible/mine validation may compile
/// their own derived timelines; those operations are deliberately not counted
/// as original compilation. Shared assets count obtained immutable handles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreparationCounters {
    pub parses: usize,
    pub compiles: usize,
    pub reads: usize,
    pub decodes: usize,
    pub shared_assets: usize,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreparationRetainedBytes {
    pub encoded_bytes: usize,
    pub pcm_bytes: usize,
    pub chart_bytes: usize,
    pub plan_items: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct PreparationCacheLimits {
    pub max_encoded_bytes: usize,
    pub max_pcm_bytes: usize,
    pub max_chart_bytes: usize,
}
impl Default for PreparationCacheLimits {
    fn default() -> Self {
        Self {
            max_encoded_bytes: 256 * 1024 * 1024,
            max_pcm_bytes: 256 * 1024 * 1024,
            max_chart_bytes: 32 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Debug)]
pub struct PreparationRequest {
    pub source_identity: String,
    pub seed: u64,
    pub format: AudioFormat,
    pub limits: PcmLimits,
    pub channels: ChannelPolicy,
    pub paths: AssetPathPolicy,
}
fn same_request(a: &PreparationRequest, b: &PreparationRequest) -> bool {
    a.source_identity == b.source_identity
        && a.seed == b.seed
        && a.format == b.format
        && a.channels == b.channels
        && a.paths == b.paths
        && a.limits.max_asset_bytes() == b.limits.max_asset_bytes()
        && a.limits.max_total_bytes() == b.limits.max_total_bytes()
        && a.limits.max_samples() == b.limits.max_samples()
}
/// Immutable parsed/compiled originals; no transport, judge, or section state.
pub struct ParsedPreparation {
    source: BmsChart,
    compiled: CompiledBms,
    referenced: BTreeSet<SampleId>,
    gain: f32,
}
impl ParsedPreparation {
    pub fn source(&self) -> &BmsChart {
        &self.source
    }
    pub fn compiled(&self) -> &CompiledBms {
        &self.compiled
    }
    pub fn referenced_samples(&self) -> &BTreeSet<SampleId> {
        &self.referenced
    }
    fn items(&self) -> usize {
        self.source.notes.len()
            + self.source.invisible.len()
            + self.compiled.chart.objects().len()
            + self.compiled.bgm.len()
    }
}
/// Owned query continuation. It retains only immutable chart bytes and the
/// canonical parsed plan, never resource IO, PCM, media, or gameplay state.
#[derive(Clone)]
pub struct PreparationPlan {
    source_identity: String,
    seed: u64,
    chart: Arc<Vec<u8>>,
    parsed: Arc<ParsedPreparation>,
}
struct CachedResource {
    encoded: Arc<Vec<u8>>,
    sample: SampleId,
}
struct CachedOriginal {
    request: PreparationRequest,
    chart: Arc<Vec<u8>>,
    parsed: Arc<ParsedPreparation>,
    bank: SampleBank,
    resources: BTreeMap<PathBuf, CachedResource>,
    retained: PreparationRetainedBytes,
}
pub struct PreparationCache {
    limits: PreparationCacheLimits,
    identity: Arc<()>,
    original: Option<Arc<CachedOriginal>>,
}
impl PreparationCache {
    pub fn new(limits: PreparationCacheLimits) -> Result<Self> {
        if limits.max_encoded_bytes == 0
            || limits.max_pcm_bytes == 0
            || limits.max_chart_bytes == 0
            || limits.max_encoded_bytes > 256 * 1024 * 1024
            || limits.max_pcm_bytes > 256 * 1024 * 1024
            || limits.max_chart_bytes > 32 * 1024 * 1024
        {
            return Err("invalid original cache limits".into());
        }
        Ok(Self {
            limits,
            identity: Arc::new(()),
            original: None,
        })
    }
    pub fn begin(
        &self,
        request: PreparationRequest,
        chart_bytes: Vec<u8>,
    ) -> Result<PreparationJob> {
        let previous = self
            .original
            .as_ref()
            .filter(|old| {
                same_request(&old.request, &request)
                    && old.chart.as_slice() == chart_bytes.as_slice()
            })
            .cloned();
        let mut job = PreparationJob::uncached(request, chart_bytes)?;
        job.cache_identity = Some(Arc::clone(&self.identity));
        job.cache_limits = Some(self.limits);
        job.previous = previous;
        job.retain_identity = job.chart.len() <= self.limits.max_chart_bytes
            && job.chart.len() <= self.limits.max_encoded_bytes
            && job.request.source_identity.len() <= ParseOptions::default().max_bytes;
        Ok(job)
    }
    /// Resume an exact query plan; changed source/seed/bytes take the normal
    /// cold path. PCM reuse still requires the complete original request key.
    pub fn begin_with_plan(
        &self,
        request: PreparationRequest,
        chart_bytes: Vec<u8>,
        plan: &PreparationPlan,
    ) -> Result<PreparationJob> {
        let exact = request.source_identity == plan.source_identity
            && request.seed == plan.seed
            && chart_bytes.as_slice() == plan.chart.as_slice();
        let mut job = self.begin(request, chart_bytes)?;
        if exact {
            job.reused_plan = Some(Arc::clone(&plan.parsed));
            job.chart = Arc::clone(&plan.chart);
        }
        Ok(job)
    }
    /// Call only after current request-owner admission. A candidate is committed once.
    pub fn commit(&mut self, candidate: &PreparationCandidate) -> Result<bool> {
        self.commit_admission(&candidate.admission)
    }
    /// Publish an original only after fresh section/policy/replay/media checks.
    /// Splitting the prepared value does not bypass cancellation or one-shot use.
    pub fn commit_admission(&mut self, admission: &PreparationAdmission) -> Result<bool> {
        if admission.committed.get()
            || !admission.permit.load(Ordering::Acquire)
            || admission
                .cache_identity
                .as_ref()
                .is_some_and(|id| !Arc::ptr_eq(id, &self.identity))
        {
            return Err("foreign or already committed preparation candidate".into());
        }
        admission.committed.set(true);
        if let Some(original) = &admission.original {
            self.original = Some(Arc::clone(original));
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub fn retained_bytes(&self) -> PreparationRetainedBytes {
        self.original
            .as_ref()
            .map_or(PreparationRetainedBytes::default(), |v| v.retained)
    }
    pub fn clear(&mut self) {
        self.original = None;
        self.identity = Arc::new(());
    }
}
pub enum PreparationAdvance {
    Pending(PreparationProgress),
    Ready(PreparationCandidate),
}
/// A deferred, one-shot cache publication permit. Dropping it publishes nothing.
pub struct PreparationAdmission {
    original: Option<Arc<CachedOriginal>>,
    cache_identity: Option<Arc<()>>,
    committed: Cell<bool>,
    permit: Arc<AtomicBool>,
}
pub struct PreparationCandidate {
    prepared: PreparedBms,
    admission: PreparationAdmission,
    counters: PreparationCounters,
}
impl PreparationCandidate {
    pub fn prepared(&self) -> &PreparedBms {
        &self.prepared
    }
    pub fn into_prepared(self) -> PreparedBms {
        self.prepared
    }
    pub fn counters(&self) -> PreparationCounters {
        self.counters
    }
    pub fn into_prepared_and_admission(self) -> (PreparedBms, PreparationAdmission) {
        (self.prepared, self.admission)
    }
}
pub struct PreparationJob {
    request: PreparationRequest,
    chart: Arc<Vec<u8>>,
    source: Option<BmsChart>,
    parsed: Option<Arc<ParsedPreparation>>,
    previous: Option<Arc<CachedOriginal>>,
    reused_plan: Option<Arc<ParsedPreparation>>,
    cache_identity: Option<Arc<()>>,
    cache_limits: Option<PreparationCacheLimits>,
    retain_identity: bool,
    resources: BTreeMap<PathBuf, CachedResource>,
    retained_encoded: usize,
    retained_pcm: usize,
    bank: Option<SampleBank>,
    decoded: BTreeMap<PathBuf, SampleId>,
    samples: Vec<SampleId>,
    index: usize,
    key: Option<PathBuf>,
    encoded: Option<Arc<Vec<u8>>>,
    cached_sample: Option<SampleId>,
    pcm: Option<PcmSample>,
    stage: PreparationStage,
    terminal: bool,
    counters: PreparationCounters,
    permit: Arc<AtomicBool>,
}
impl PreparationJob {
    pub fn uncached(request: PreparationRequest, chart_bytes: Vec<u8>) -> Result<Self> {
        Ok(Self {
            request,
            retained_encoded: chart_bytes.len(),
            chart: Arc::new(chart_bytes),
            source: None,
            parsed: None,
            previous: None,
            reused_plan: None,
            cache_identity: None,
            cache_limits: None,
            retain_identity: false,
            resources: BTreeMap::new(),
            retained_pcm: 0,
            bank: None,
            decoded: BTreeMap::new(),
            samples: Vec::new(),
            index: 0,
            key: None,
            encoded: None,
            cached_sample: None,
            pcm: None,
            stage: PreparationStage::Parse,
            terminal: false,
            counters: PreparationCounters::default(),
            permit: Arc::new(AtomicBool::new(true)),
        })
    }
    pub fn progress(&self) -> PreparationProgress {
        PreparationProgress {
            stage: self.stage,
            completed_assets: self.index,
            total_assets: self.samples.len(),
        }
    }
    pub fn counters(&self) -> PreparationCounters {
        self.counters
    }
    pub fn parsed(&self) -> Option<&ParsedPreparation> {
        self.parsed.as_deref()
    }
    pub fn shared_plan(&self) -> Option<PreparationPlan> {
        self.parsed.as_ref().map(|parsed| PreparationPlan {
            source_identity: self.request.source_identity.clone(),
            seed: self.request.seed,
            chart: Arc::clone(&self.chart),
            parsed: Arc::clone(parsed),
        })
    }
    pub fn pending_resource(&self) -> Option<&Path> {
        self.key.as_deref()
    }
    pub fn cancel(&mut self) {
        if !self.permit.swap(false, Ordering::AcqRel) {
            return;
        }
        self.terminal = true;
        self.chart = Arc::new(Vec::new());
        self.encoded = None;
        self.pcm = None;
        self.bank = None;
        self.resources.clear();
        self.previous = None;
        self.reused_plan = None;
        self.source = None;
        self.parsed = None;
    }
    pub fn advance(
        &mut self,
        assets: &dyn AssetSource,
        replay: Option<(&ReplayFile, ReplayCodecLimits)>,
        control: &mut dyn FnMut(PreparationProgress) -> Result<()>,
    ) -> Result<PreparationAdvance> {
        self.advance_inner(assets, &DefaultAssetDecoder, replay, control)
    }
    pub fn advance_with_decoder(
        &mut self,
        assets: &dyn AssetSource,
        decoder: &dyn AssetDecoder,
        replay: Option<(&ReplayFile, ReplayCodecLimits)>,
        control: &mut dyn FnMut(PreparationProgress) -> Result<()>,
    ) -> Result<PreparationAdvance> {
        if self.cache_identity.is_some() {
            self.cancel();
            return Err("custom decoder preparation is explicitly uncached".into());
        }
        self.advance_inner(assets, decoder, replay, control)
    }
    fn advance_inner(
        &mut self,
        assets: &dyn AssetSource,
        decoder: &dyn AssetDecoder,
        replay: Option<(&ReplayFile, ReplayCodecLimits)>,
        control: &mut dyn FnMut(PreparationProgress) -> Result<()>,
    ) -> Result<PreparationAdvance> {
        if self.terminal {
            return Err("preparation job is terminal".into());
        }
        let result = self.step(assets, decoder, replay, control);
        if result.is_err() {
            self.cancel();
        }
        result
    }
    fn step(
        &mut self,
        assets: &dyn AssetSource,
        decoder: &dyn AssetDecoder,
        replay: Option<(&ReplayFile, ReplayCodecLimits)>,
        control: &mut dyn FnMut(PreparationProgress) -> Result<()>,
    ) -> Result<PreparationAdvance> {
        let progress = self.progress();
        control(progress)?;
        match self.stage {
            PreparationStage::Parse => {
                if self.reused_plan.is_none() && self.previous.is_none() {
                    let options = ParseOptions::default();
                    let text = crate::chart_text::decode_chart_text(
                        &self.chart,
                        crate::chart_text::ChartTextEncoding::Auto,
                        options.max_bytes,
                    )?;
                    self.source = Some(beatkernel_bms::parse_seeded(
                        &text,
                        options,
                        self.request.seed,
                    )?);
                    self.counters.parses += 1;
                }
                self.stage = PreparationStage::Compile;
            }
            PreparationStage::Compile => {
                let reusable = self
                    .reused_plan
                    .as_ref()
                    .or_else(|| self.previous.as_ref().map(|old| &old.parsed));
                let source = reusable
                    .map(|plan| &plan.source)
                    .or(self.source.as_ref())
                    .ok_or("parsed preparation missing")?;
                let gain = source.wav_gain()?;
                let invisible = if source.invisible.is_empty() {
                    Vec::new()
                } else {
                    source.compile_invisible()?
                };
                if let Some((file, limits)) = replay {
                    crate::replay_playback::validate_section_setup(source, file, limits)?;
                }
                let compiled = if reusable.is_none() {
                    let compiled = source.compile()?;
                    self.counters.compiles += 1;
                    Some(compiled)
                } else {
                    None
                };
                let actual = reusable
                    .map(|plan| &plan.compiled)
                    .or(compiled.as_ref())
                    .unwrap();
                let referenced = referenced_samples(
                    source,
                    actual,
                    &invisible,
                    self.request.limits.max_samples(),
                )?;
                self.samples = referenced.iter().copied().collect();
                self.parsed = Some(if let Some(plan) = reusable {
                    Arc::clone(plan)
                } else {
                    Arc::new(ParsedPreparation {
                        source: self.source.take().unwrap(),
                        compiled: compiled.unwrap(),
                        referenced,
                        gain,
                    })
                });
                self.bank = Some(SampleBank::new(self.request.format, self.request.limits)?);
                self.stage = if self.samples.is_empty() {
                    PreparationStage::Ready
                } else {
                    PreparationStage::Resolve
                };
            }
            PreparationStage::Resolve => {
                let sample = self.samples[self.index];
                let name = self
                    .parsed
                    .as_ref()
                    .unwrap()
                    .source
                    .samples
                    .get(&u16::try_from(sample.0)?)
                    .ok_or("referenced sample has no WAV definition")?;
                let key = assets.resolve(name, self.request.paths)?;
                if let Some(&first) = self.decoded.get(&key) {
                    self.pcm = Some(
                        self.bank
                            .as_ref()
                            .unwrap()
                            .get(first)
                            .ok_or("prepared alias missing")?
                            .try_share(self.request.limits)?,
                    );
                    self.counters.shared_assets += 1;
                    self.key = Some(key);
                    self.stage = PreparationStage::Admit;
                } else {
                    self.key = Some(key);
                    self.stage = PreparationStage::Read;
                }
            }
            PreparationStage::Read => {
                let encoded = assets.read(self.key.as_deref().unwrap(), 64 * 1024 * 1024)?;
                if encoded.len() > 64 * 1024 * 1024 {
                    return Err("encoded file exceeds preparation limit".into());
                }
                self.counters.reads += 1;
                let previous = self
                    .previous
                    .as_ref()
                    .and_then(|old| old.resources.get(self.key.as_ref().unwrap()))
                    .filter(|old| old.encoded.as_slice() == encoded.as_ref());
                if let Some(previous) = previous {
                    self.encoded = Some(Arc::clone(&previous.encoded));
                    self.cached_sample = Some(previous.sample);
                } else {
                    let bytes = match encoded {
                        std::borrow::Cow::Owned(bytes) => bytes,
                        std::borrow::Cow::Borrowed(bytes) => {
                            let mut owned = Vec::new();
                            owned.try_reserve_exact(bytes.len())?;
                            owned.extend_from_slice(bytes);
                            owned
                        }
                    };
                    self.encoded = Some(Arc::new(bytes));
                    self.cached_sample = None;
                }
                self.stage = PreparationStage::Decode;
            }
            PreparationStage::Decode => {
                let key = self.key.as_ref().unwrap();
                let encoded = self.encoded.as_ref().unwrap();
                if let Some(sample) = self.cached_sample.take() {
                    self.pcm = Some(
                        self.previous
                            .as_ref()
                            .unwrap()
                            .bank
                            .get(sample)
                            .ok_or("cached original PCM missing")?
                            .try_share(self.request.limits)?,
                    );
                    self.counters.shared_assets += 1;
                } else {
                    self.pcm =
                        Some(decoder.decode(key, encoded.as_slice(), self.request.limits)?);
                    self.counters.decodes += 1;
                }
                self.stage = PreparationStage::Channels;
            }
            PreparationStage::Channels => {
                self.pcm = Some(crate::prepare_channels(
                    self.pcm.take().unwrap(),
                    self.request.format,
                    self.request.limits,
                    self.request.channels,
                )?);
                self.stage = PreparationStage::Admit;
            }
            PreparationStage::Admit => {
                let sample = self.samples[self.index];
                let key = self.key.take().unwrap();
                let unique = !self.decoded.contains_key(&key);
                let pcm = self.pcm.take().unwrap();
                if unique {
                    self.retained_pcm = self
                        .retained_pcm
                        .checked_add(
                            pcm.samples()
                                .len()
                                .checked_mul(4)
                                .ok_or("PCM byte overflow")?,
                        )
                        .ok_or("PCM total overflow")?;
                }
                self.bank.as_mut().unwrap().insert(sample, pcm)?;
                if unique {
                    self.decoded.insert(key.clone(), sample);
                    let encoded = self.encoded.take().unwrap();
                    if self.retain_identity {
                        let limits = self.cache_limits.unwrap();
                        let total = self.retained_encoded.checked_add(encoded.len());
                        if total.is_some_and(|v| v <= limits.max_encoded_bytes)
                            && self.retained_pcm
                                <= limits
                                    .max_pcm_bytes
                                    .min(self.request.limits.max_total_bytes())
                        {
                            self.retained_encoded = total.unwrap();
                            self.resources
                                .insert(key, CachedResource { encoded, sample });
                        } else {
                            self.retain_identity = false;
                            self.resources.clear();
                        }
                    }
                }
                self.index += 1;
                self.stage = if self.index == self.samples.len() {
                    PreparationStage::Ready
                } else {
                    PreparationStage::Resolve
                };
            }
            PreparationStage::Ready => {
                let parsed = if self.cache_identity.is_none() {
                    self.parsed.take().unwrap()
                } else {
                    Arc::clone(self.parsed.as_ref().unwrap())
                };
                let (sounds, bgm_commands) = bindings(&parsed)?;
                let bank = self.bank.take().unwrap();
                let original = if self.retain_identity {
                    Some(Arc::new(CachedOriginal {
                        request: self.request.clone(),
                        chart: Arc::clone(&self.chart),
                        parsed: Arc::clone(&parsed),
                        bank: bank.try_share(self.request.limits)?,
                        resources: std::mem::take(&mut self.resources),
                        retained: PreparationRetainedBytes {
                            encoded_bytes: self.retained_encoded,
                            pcm_bytes: self.retained_pcm,
                            chart_bytes: self.chart.len(),
                            plan_items: parsed.items(),
                        },
                    }))
                } else {
                    None
                };
                let (source, compiled) = match Arc::try_unwrap(parsed) {
                    Ok(parsed) => (parsed.source, parsed.compiled),
                    Err(parsed) => (parsed.source.clone(), parsed.compiled.clone()),
                };
                let prepared = PreparedBms {
                    source,
                    compiled,
                    bank,
                    sounds,
                    bgm_commands,
                };
                control(progress)?;
                self.terminal = true;
                return Ok(PreparationAdvance::Ready(PreparationCandidate {
                    prepared,
                    admission: PreparationAdmission {
                        original,
                        cache_identity: self.cache_identity.clone(),
                        committed: Cell::new(false),
                        permit: Arc::clone(&self.permit),
                    },
                    counters: self.counters,
                }));
            }
            PreparationStage::Visual => {
                return Err("visual stage belongs to fresh media jobs".into())
            }
        }
        control(progress)?;
        Ok(PreparationAdvance::Pending(self.progress()))
    }
}
fn bindings(parsed: &ParsedPreparation) -> Result<(Vec<SoundBinding>, Vec<AudioCommand>)> {
    let objects: BTreeMap<_, _> = parsed
        .compiled
        .chart
        .objects()
        .iter()
        .map(|object| (object.id, object))
        .collect();
    let mut sounds = Vec::new();
    sounds.try_reserve_exact(parsed.source.notes.len())?;
    for note in &parsed.source.notes {
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
            gain: parsed.gain,
        });
    }
    let mut bgm_commands = Vec::new();
    bgm_commands.try_reserve_exact(parsed.compiled.bgm.len())?;
    if !parsed.compiled.bgm.is_empty() {
        let first = objects
            .keys()
            .map(|id| id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("BGM voice identity overflow")?;
        for (index, event) in parsed.compiled.bgm.iter().enumerate() {
            let voice = first
                .checked_add(u64::try_from(index)?)
                .ok_or("BGM voice identity overflow")?;
            bgm_commands.push(AudioCommand::Play {
                voice: VoiceId(voice),
                sample: event.sample,
                at: event.at,
                gain: parsed.gain,
            });
        }
    }
    Ok((sounds, bgm_commands))
}
/// Compatibility path: arbitrary decoder implementations are never cached.
#[allow(clippy::too_many_arguments)]
pub fn prepare_uncached(
    chart_bytes: &[u8],
    assets: &dyn AssetSource,
    format: AudioFormat,
    pcm_limits: PcmLimits,
    channels: ChannelPolicy,
    decoder: &dyn AssetDecoder,
    paths: AssetPathPolicy,
    seed: u64,
    replay: Option<(&ReplayFile, ReplayCodecLimits)>,
) -> Result<PreparedBms> {
    if chart_bytes.len() > ParseOptions::default().max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "raw chart text exceeds byte limit",
        )
        .into());
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(chart_bytes.len())?;
    bytes.extend_from_slice(chart_bytes);
    let mut job = PreparationJob::uncached(
        PreparationRequest {
            source_identity: String::new(),
            seed,
            format,
            limits: pcm_limits,
            channels,
            paths,
        },
        bytes,
    )?;
    loop {
        if let PreparationAdvance::Ready(candidate) =
            job.advance_with_decoder(assets, decoder, replay, &mut |_| Ok(()))?
        {
            return Ok(candidate.into_prepared());
        }
    }
}
