//! Prepared native movie IO owner and a bounded, nonblocking render-side feed.
use crate::{
    video::{
        VideoDecodeEvent, VideoDecoderPort, VideoFrame, VideoFrameAdmission, VideoFrameLimits,
        VideoFrameQueue, VideoSessionKey,
    },
    video_assets::{VideoAssetLimits, VideoAssets, VideoResource, VideoTransform},
    video_native::{FfmpegDecoder, FfmpegDecoderConfig},
};
use beatkernel::time::Timestamp;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::io::AsyncReadExt;
use tokio::sync::watch;

const SLOTS: usize = 16;
/// Aggregate decoded CPU ownership across active and retiring channel streams.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeVideoBankLimits {
    pub max_working_bytes: u64,
}
impl Default for NativeVideoBankLimits {
    fn default() -> Self {
        Self {
            max_working_bytes: 512 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeVideoReservation {
    pub raw_bytes: u64,
    pub transformed_bytes: u64,
    pub output_bytes: u64,
    pub decoder_working_bytes: u64,
    pub working_bytes: u64,
}
impl NativeVideoReservation {
    pub fn for_extent(width: u32, height: u32, transform: VideoTransform) -> Result<Self, String> {
        let limits = VideoAssetLimits::default();
        if let Some(crop) = transform.crop {
            crop.validate().map_err(|error| error.to_string())?;
            if transform.canvas.is_none() {
                return Err("movie crop requires canvas".into());
            }
        }
        let raw_bytes = limits.validate_frame(width, height)?;
        let [output_width, output_height] = transform.canvas.unwrap_or([width, height]);
        let output_bytes = limits.validate_frame(output_width, output_height)?;
        let transformed_bytes = output_bytes
            .checked_mul(if transform.keyed { 2 } else { 1 })
            .ok_or("movie transform budget overflow")?;
        if transformed_bytes > limits.max_frame_bytes {
            return Err("movie transform variants exceed frame limit".into());
        }
        let decoder_working_bytes = raw_bytes
            .max(transformed_bytes)
            .checked_mul(4)
            .ok_or("movie decoder budget overflow")?;
        // Decoder temporary/read-ahead ownership plus manager pending, ready,
        // and all three render-side retained frames; Arc aliases count once.
        let working_bytes = output_bytes
            .checked_mul(5)
            .and_then(|external| decoder_working_bytes.checked_add(external))
            .ok_or("movie stream budget overflow")?;
        Ok(Self {
            raw_bytes,
            transformed_bytes,
            output_bytes,
            decoder_working_bytes,
            working_bytes,
        })
    }
    /// Derive stream-local limits without expanding any caller-owned ceiling.
    pub(crate) fn decoder_config(
        self,
        config: &FfmpegDecoderConfig,
    ) -> Result<FfmpegDecoderConfig, String> {
        config.frame_limits.validate()?;
        let retained_bytes = self
            .output_bytes
            .checked_mul(3)
            .ok_or("movie retained budget overflow")?;
        if self.raw_bytes.max(self.transformed_bytes) > config.asset_limits.max_frame_bytes
            || self.output_bytes > config.frame_limits.max_frame_bytes
            || self.decoder_working_bytes > config.max_working_bytes
            || retained_bytes > config.frame_limits.max_bytes
            || config.frame_limits.max_frames < 3
        {
            return Err("movie stream exceeds configured frame/working limits".into());
        }
        Ok(FfmpegDecoderConfig {
            max_working_bytes: self.decoder_working_bytes,
            asset_limits: VideoAssetLimits {
                max_frame_bytes: self.raw_bytes.max(self.transformed_bytes),
                ..config.asset_limits
            },
            frame_limits: VideoFrameLimits {
                max_frames: 3,
                max_bytes: retained_bytes,
                max_frame_bytes: self.output_bytes,
            },
            ..config.clone()
        })
    }
}
pub struct NativeVideoBudget {
    limits: NativeVideoBankLimits,
    reservations: [u64; SLOTS],
    used: u64,
}
impl NativeVideoBudget {
    pub fn new(limits: NativeVideoBankLimits) -> Result<Self, String> {
        if limits.max_working_bytes == 0 {
            return Err("movie bank working budget must be positive".into());
        }
        Ok(Self {
            limits,
            reservations: [0; SLOTS],
            used: 0,
        })
    }
    pub fn reserve(
        &mut self,
        slot: usize,
        reservation: NativeVideoReservation,
    ) -> Result<(), String> {
        let old = *self
            .reservations
            .get(slot)
            .ok_or("movie slot exceeds capacity")?;
        let used = self
            .used
            .checked_sub(old)
            .and_then(|used| used.checked_add(reservation.working_bytes))
            .ok_or("movie bank budget overflow")?;
        if reservation.working_bytes == 0 || used > self.limits.max_working_bytes {
            return Err("movie bank working budget exhausted".into());
        }
        self.reservations[slot] = reservation.working_bytes;
        self.used = used;
        Ok(())
    }
    pub fn release(&mut self, slot: usize) {
        if let Some(bytes) = self.reservations.get_mut(slot) {
            self.used -= *bytes;
            *bytes = 0;
        }
    }
    pub fn used_bytes(&self) -> u64 {
        self.used
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Demand {
    session: VideoSessionKey,
    target: Timestamp,
}
struct Output {
    demand: Demand,
    event: VideoDecodeEvent,
}
/// Cloning snapshots clones this handle, never frame pixels. The service owns
/// decoder construction, filesystem capability discovery and every join.
pub struct NativeVideoBank {
    pub assets: Arc<VideoAssets>,
    content: u64,
    demands: Vec<watch::Sender<Option<Demand>>>,
    ready: Vec<Arc<Mutex<mpsc::Receiver<Output>>>>,
    shutdown: watch::Sender<bool>,
    owner: Mutex<Option<std::thread::JoinHandle<Result<(), String>>>>,
}
impl NativeVideoBank {
    pub fn prepare(assets: Arc<VideoAssets>) -> Result<Arc<Self>, String> {
        Self::prepare_with_config(
            assets,
            NativeVideoBankLimits::default(),
            FfmpegDecoderConfig::default(),
        )
    }
    pub fn prepare_with_config(
        assets: Arc<VideoAssets>,
        limits: NativeVideoBankLimits,
        config: FfmpegDecoderConfig,
    ) -> Result<Arc<Self>, String> {
        let budget = NativeVideoBudget::new(limits)?;
        config.asset_limits.validate()?;
        config.frame_limits.validate()?;
        if config.max_working_bytes == 0 || config.frame_limits.max_frames < 3 {
            return Err("movie decoder working/retained limits invalid".into());
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let content = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| "video content identity exhausted")?;
        let mut demands = Vec::with_capacity(SLOTS);
        let mut inputs = Vec::with_capacity(SLOTS);
        let mut ready = Vec::with_capacity(SLOTS);
        let mut outputs = Vec::with_capacity(SLOTS);
        for _ in 0..SLOTS {
            let (tx, rx) = watch::channel(None);
            demands.push(tx);
            inputs.push(rx);
            let (tx, rx) = mpsc::sync_channel(1);
            outputs.push(tx);
            ready.push(Arc::new(Mutex::new(rx)));
        }
        let (shutdown, stop) = watch::channel(false);
        let owner_assets = Arc::clone(&assets);
        let owner_ready = ready.clone();
        let owner = std::thread::Builder::new()
            .name("video-bank-io".into())
            .spawn(move || {
                service(
                    owner_assets,
                    inputs,
                    outputs,
                    owner_ready,
                    stop,
                    budget,
                    config,
                    runtime,
                )
            })
            .map_err(|error| error.to_string())?;
        Ok(Arc::new(Self {
            assets,
            content,
            demands,
            ready,
            shutdown,
            owner: Mutex::new(Some(owner)),
        }))
    }
    pub fn content(&self) -> u64 {
        self.content
    }
    pub fn request(
        &self,
        slot: usize,
        session: VideoSessionKey,
        target: Timestamp,
    ) -> Result<(), String> {
        if *self.shutdown.borrow() {
            return Err("movie bank is shut down".into());
        }
        if session.content != self.content {
            return Err("movie demand belongs to another content".into());
        }
        let sender = self
            .demands
            .get(slot)
            .ok_or("movie slot exceeds capacity")?;
        if self.assets.get(session.image).is_none() {
            return Err("movie image descriptor unavailable".into());
        }
        sender.send_replace(Some(Demand { session, target }));
        Ok(())
    }
    pub fn try_next(&self, slot: usize) -> Option<VideoDecodeEvent> {
        if *self.shutdown.borrow() {
            return None;
        }
        let current = *self.demands.get(slot)?.borrow();
        let receiver = self.ready.get(slot)?.try_lock().ok()?;
        while let Ok(output) = receiver.try_recv() {
            if current.is_some_and(|demand| demand.session == output.demand.session) {
                return Some(output.event);
            }
        }
        None
    }
    pub fn retire(&self, slot: usize) {
        if let Some(sender) = self.demands.get(slot) {
            sender.send_replace(None);
            if let Some(receiver) = self.ready.get(slot).and_then(|ready| ready.try_lock().ok()) {
                while receiver.try_recv().is_ok() {}
            }
        }
    }
    pub fn retire_all(&self) {
        for slot in 0..SLOTS {
            self.retire(slot);
        }
    }
    /// Cancel immediately; no filesystem, decoder or thread wait occurs here.
    pub fn shutdown(&self) {
        self.shutdown.send_replace(true);
        self.retire_all();
    }
    /// Join the service and all decoder children on the game cleanup owner.
    /// Never call this from input, audio, render or UI callbacks.
    pub fn join(&self) -> Result<(), String> {
        self.shutdown();
        let mut owner = self.owner.lock().map_err(|_| "movie owner lock poisoned")?;
        if let Some(worker) = owner.take() {
            worker
                .join()
                .map_err(|_| "movie service owner panicked")??;
        }
        Ok(())
    }
}
impl Drop for NativeVideoBank {
    fn drop(&mut self) {
        // Snapshots may release their last Arc on the render owner. The game
        // Session explicitly joins before publishing its terminal snapshot.
        self.shutdown();
    }
}
fn service(
    assets: Arc<VideoAssets>,
    inputs: Vec<watch::Receiver<Option<Demand>>>,
    outputs: Vec<mpsc::SyncSender<Output>>,
    ready: Vec<Arc<Mutex<mpsc::Receiver<Output>>>>,
    stop: watch::Receiver<bool>,
    mut budget: NativeVideoBudget,
    config: FfmpegDecoderConfig,
    runtime: tokio::runtime::Runtime,
) -> Result<(), String> {
    let probe = find_probe(config.executable.as_deref());
    let mut extents: BTreeMap<usize, [u32; 2]> = BTreeMap::new();
    let mut decoders: Vec<Option<FfmpegDecoder>> = (0..SLOTS).map(|_| None).collect();
    let mut active: [Option<Demand>; SLOTS] = [None; SLOTS];
    let mut pending: [Option<Output>; SLOTS] = std::array::from_fn(|_| None);
    let mut cleanup_error = None;
    'owner: while !*stop.borrow() {
        for slot in 0..SLOTS {
            let demand = *inputs[slot].borrow();
            let changed_session = active[slot]
                .is_some_and(|old| demand.is_some_and(|new| old.session != new.session));
            let changed_resource = active[slot].is_some_and(|old| {
                demand.is_some_and(|new| {
                    assets.get(old.session.image) != assets.get(new.session.image)
                })
            });
            if demand.is_none() || changed_resource {
                if let Some(mut decoder) = decoders[slot].take() {
                    if let Err(reason) = decoder.join() {
                        cleanup_error.get_or_insert(reason);
                    }
                }
                pending[slot] = None;
                if let Ok(receiver) = ready[slot].lock() {
                    while receiver.try_recv().is_ok() {}
                }
                // Retirement remains charged until all owned pixels and decoder joins clear.
                budget.release(slot);
                active[slot] = None;
            } else if changed_session {
                // The decoder retains the original presentation origin across seek.
                // request() cancels/reaps its old child on the decoder IO owner.
                pending[slot] = None;
            }
            let Some(demand) = demand else {
                continue;
            };
            if active[slot] != Some(demand) {
                let result = (|| {
                    if decoders[slot].is_none() {
                        let descriptor = assets
                            .get(demand.session.image)
                            .ok_or("movie descriptor unavailable")?;
                        let resource = assets
                            .resource(descriptor.resource)
                            .ok_or("movie resource unavailable")?
                            .clone();
                        let extent = if let Some(extent) = extents.get(&descriptor.resource) {
                            *extent
                        } else {
                            let executable = probe.as_ref().ok_or("movie dimension probe unavailable; install FFprobe alongside FFmpeg")?;
                            let VideoResource::File(path) = &resource.data else {
                                return Err("native movie dimension probe requires a file".into());
                            };
                            let extent = runtime.block_on(probe_extent(
                                executable,
                                path,
                                inputs[slot].clone(),
                                stop.clone(),
                                demand,
                            ))?;
                            extents.insert(descriptor.resource, extent);
                            extent
                        };
                        let reservation = NativeVideoReservation::for_extent(
                            extent[0],
                            extent[1],
                            descriptor.transform,
                        )?;
                        let decoder_config = reservation.decoder_config(&config)?;
                        budget.reserve(slot, reservation)?;
                        match FfmpegDecoder::open(resource, descriptor.transform, decoder_config) {
                            Ok(decoder) => decoders[slot] = Some(decoder),
                            Err(reason) => {
                                budget.release(slot);
                                return Err(reason);
                            }
                        }
                    }
                    decoders[slot]
                        .as_mut()
                        .unwrap()
                        .request(demand.session, demand.target)
                })();
                active[slot] = Some(demand);
                if let Err(reason) = result {
                    pending[slot] = Some(Output {
                        demand,
                        event: VideoDecodeEvent::Failed {
                            session: demand.session,
                            reason,
                        },
                    });
                }
            }
            if pending[slot].is_none() {
                pending[slot] = decoders[slot]
                    .as_mut()
                    .and_then(|decoder| decoder.try_next())
                    .map(|event| Output { demand, event });
            }
            if let Some(output) = pending[slot].take() {
                match outputs[slot].try_send(output) {
                    Ok(()) => {}
                    Err(mpsc::TrySendError::Full(output)) => pending[slot] = Some(output),
                    Err(mpsc::TrySendError::Disconnected(_)) => break 'owner,
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    for mut decoder in decoders.into_iter().flatten() {
        if let Err(reason) = decoder.join() {
            cleanup_error.get_or_insert(reason);
        }
    }
    cleanup_error.map_or(Ok(()), Err)
}
fn find_probe(override_path: Option<&Path>) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    };
    let sibling = override_path
        .and_then(Path::parent)
        .map(|parent| parent.join(name));
    sibling.filter(|path| path.is_file()).or_else(|| {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|path| path.join(name))
                .find(|path| path.is_file())
        })
    })
}
async fn probe_extent(
    executable: &Path,
    path: &Path,
    demand_rx: watch::Receiver<Option<Demand>>,
    stop: watch::Receiver<bool>,
    demand: Demand,
) -> Result<[u32; 2], String> {
    let mut child = tokio::process::Command::new(executable)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0:s=x",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("movie dimension probe launch: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or("movie dimension probe stdout unavailable")?;
    let mut bytes = Vec::new();
    let result: Result<(), String> = {
        let read = async {
            stdout
                .take(4097)
                .read_to_end(&mut bytes)
                .await
                .map_err(|error| error.to_string())?;
            if bytes.len() > 4096 {
                return Err("movie dimension probe exceeds output limit".into());
            }
            let status = child.wait().await.map_err(|error| error.to_string())?;
            if !status.success() {
                return Err("movie dimension probe failed".into());
            }
            Ok(())
        };
        tokio::pin!(read);
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(5));
        tokio::pin!(timeout);
        loop {
            tokio::select! {
                result = &mut read => break result,
                _ = &mut timeout => break Err("movie dimension probe timed out".into()),
                _ = tokio::time::sleep(std::time::Duration::from_millis(20)) => {
                    if *stop.borrow() || (*demand_rx.borrow()).is_none_or(|current| current.session != demand.session) { break Err("movie dimension probe cancelled".into()); }
                }
            }
        }
    };
    if result.is_err() {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
    result?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "movie dimension probe text invalid")?;
    let (width, height) = text
        .trim()
        .split_once('x')
        .ok_or("movie dimension probe extent missing")?;
    let width = width
        .parse()
        .map_err(|_| "movie dimension probe width invalid")?;
    let height = height
        .parse()
        .map_err(|_| "movie dimension probe height invalid")?;
    VideoAssetLimits::default().validate_frame(width, height)?;
    Ok([width, height])
}
struct FrameSlot {
    session: VideoSessionKey,
    target: Timestamp,
    queue: VideoFrameQueue,
}
/// Pure feed seam: original-song demand and timestamped decoder events only.
#[derive(Default)]
pub struct NativeVideoFrames {
    slots: [Option<FrameSlot>; SLOTS],
}
impl NativeVideoFrames {
    pub fn request(
        &mut self,
        slot: usize,
        session: VideoSessionKey,
        target: Timestamp,
    ) -> Result<(), String> {
        let entry = self
            .slots
            .get_mut(slot)
            .ok_or("movie slot exceeds capacity")?;
        if let Some(old) = entry {
            if old.session == session {
                if target < old.target {
                    return Err("movie backward seek requires new generation".into());
                }
                old.target = target;
                old.queue.select(target);
                return Ok(());
            }
        }
        *entry = Some(FrameSlot {
            session,
            target,
            queue: VideoFrameQueue::new(session, VideoFrameLimits::default())?,
        });
        Ok(())
    }
    pub fn admit(&mut self, slot: usize, event: VideoDecodeEvent) -> Result<(), String> {
        let Some(entry) = self
            .slots
            .get_mut(slot)
            .ok_or("movie slot exceeds capacity")?
            .as_mut()
        else {
            return Ok(());
        };
        match event {
            VideoDecodeEvent::Frame(frame) => {
                if let VideoFrameAdmission::Backpressure(_) = entry.queue.push(frame)? {
                    return Err("movie ready queue backpressure".into());
                }
            }
            VideoDecodeEvent::Watermark { session, through } => {
                entry.queue.watermark(session, through)?;
            }
            VideoDecodeEvent::End { session, end } if session == entry.session => {
                entry.queue.finish(end)?;
            }
            VideoDecodeEvent::Failed { session, reason } if session == entry.session => {
                return Err(reason)
            }
            _ => {}
        }
        entry.queue.select(entry.target);
        Ok(())
    }
    pub fn selected(&mut self, slot: usize) -> Option<&VideoFrame> {
        let entry = self.slots.get_mut(slot)?.as_mut()?;
        entry.queue.select(entry.target)
    }
    pub fn retire(&mut self, slot: usize) {
        if let Some(entry) = self.slots.get_mut(slot) {
            *entry = None;
        }
    }
}
#[derive(Default)]
pub struct NativeVideoController {
    bank: Option<Arc<NativeVideoBank>>,
    feed: NativeVideoFrames,
    demands: [Option<Demand>; SLOTS],
    generation: u64,
    failures: [Option<(VideoSessionKey, String)>; SLOTS],
    pub unavailable: Vec<String>,
}
impl NativeVideoController {
    pub fn retire(&mut self) {
        if let Some(bank) = self.bank.take() {
            bank.retire_all();
        }
        for slot in 0..SLOTS {
            self.feed.retire(slot);
            self.demands[slot] = None;
            self.failures[slot] = None;
        }
        self.unavailable.clear();
    }
    pub fn sync(
        &mut self,
        bank: Option<&Arc<NativeVideoBank>>,
        demands: &[Option<(VideoSessionKey, Timestamp)>],
    ) -> Result<[Option<VideoFrame>; SLOTS], String> {
        if demands.len() > SLOTS {
            return Err("movie demand exceeds capacity".into());
        }
        if self.bank.as_ref().map(|old| old.content()) != bank.map(|new| new.content()) {
            self.retire();
            self.bank = bank.cloned();
        }
        let mut frames = std::array::from_fn(|_| None);
        let Some(bank) = self.bank.as_ref() else {
            return Ok(frames);
        };
        self.unavailable.clear();
        for slot in 0..SLOTS {
            let Some((mut session, target)) = demands.get(slot).copied().flatten() else {
                bank.retire(slot);
                self.feed.retire(slot);
                self.demands[slot] = None;
                self.failures[slot] = None;
                continue;
            };
            let same = self.demands[slot].is_some_and(|old| {
                let mut old_session = old.session;
                old_session.generation = session.generation;
                old_session == session && target >= old.target
            });
            if same {
                session.generation = self.demands[slot].unwrap().session.generation;
            } else {
                self.generation = self
                    .generation
                    .checked_add(1)
                    .ok_or("movie generation overflow")?;
                session.generation = self.generation;
            }
            self.demands[slot] = Some(Demand { session, target });
            if let Some((failed, reason)) = &self.failures[slot] {
                if *failed == session {
                    self.unavailable.push(reason.clone());
                    continue;
                }
            }
            self.failures[slot] = None;
            self.feed.request(slot, session, target)?;
            if let Err(reason) = bank.request(slot, session, target) {
                self.unavailable.push(reason);
                continue;
            }
            // Bounded drain, independent from gameplay snapshot ACKs.
            for _ in 0..8 {
                let Some(event) = bank.try_next(slot) else {
                    break;
                };
                if let Err(reason) = self.feed.admit(slot, event) {
                    self.failures[slot] = Some((session, reason.clone()));
                    self.unavailable.push(reason);
                    bank.retire(slot);
                    self.feed.retire(slot);
                    break;
                }
            }
            if let Some(frame) = self.feed.selected(slot) {
                frames[slot] = Some(VideoFrame {
                    session: frame.session,
                    pts: frame.pts,
                    revision: frame.revision,
                    image: Arc::clone(&frame.image),
                });
            }
        }
        Ok(frames)
    }
}
