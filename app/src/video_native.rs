//! Optional external FFmpeg owner. Only preparation and explicit join may block.
use crate::{
    texture::RgbaImage,
    video::{
        VideoDecodeEvent, VideoDecoderCapabilities, VideoDecoderPort, VideoFrame, VideoFrameLimits,
        VideoSessionKey, VideoTimeBase,
    },
    video_assets::{VideoAssetLimits, VideoResource, VideoResourceDescriptor, VideoTransform},
};
use beatkernel::time::Timestamp;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    thread::JoinHandle,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::Command,
    sync::{mpsc, watch},
};

#[derive(Clone, Debug)]
pub struct FfmpegDecoderConfig {
    pub executable: Option<PathBuf>,
    pub asset_limits: VideoAssetLimits,
    pub frame_limits: VideoFrameLimits,
    /// Peak source, transform, ready and preroll pixel ownership on the IO owner.
    pub max_working_bytes: u64,
}
impl Default for FfmpegDecoderConfig {
    fn default() -> Self {
        Self {
            executable: None,
            asset_limits: VideoAssetLimits::default(),
            frame_limits: VideoFrameLimits::default(),
            max_working_bytes: 256 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FfmpegFrameMetadata {
    pub ordinal: u64,
    pub pts: i64,
    pub time_base: VideoTimeBase,
    pub width: u32,
    pub height: u32,
    pub byte_len: usize,
}
/// Parses the filter's own integer protocol, never the rounded pts_time field.
pub struct ShowInfoParser {
    limits: VideoAssetLimits,
    time_base: Option<VideoTimeBase>,
    next: u64,
}
impl ShowInfoParser {
    pub fn new(limits: VideoAssetLimits) -> Self {
        Self {
            limits,
            time_base: None,
            next: 0,
        }
    }
    pub fn parse_line(&mut self, line: &str) -> Result<Option<FfmpegFrameMetadata>, String> {
        self.limits.validate()?;
        if line.len() > 16 * 1024 {
            return Err("FFmpeg metadata line exceeds limit".into());
        }
        if !line.contains("showinfo") {
            return Ok(None);
        }
        if let Some(rest) = line.split("config in time_base:").nth(1) {
            let ratio = rest
                .split_whitespace()
                .next()
                .ok_or("missing filter time base")?;
            let (n, d) = ratio.split_once('/').ok_or("invalid filter time base")?;
            let base = VideoTimeBase::new(
                n.trim_end_matches(',')
                    .parse()
                    .map_err(|_| "invalid filter time base")?,
                d.trim_end_matches(',')
                    .parse()
                    .map_err(|_| "invalid filter time base")?,
            )?;
            if self.time_base.is_some_and(|old| old != base) {
                return Err("filter time base changed".into());
            }
            self.time_base = Some(base);
            return Ok(None);
        }
        if !line.contains(" n:") {
            return Ok(None);
        }
        fn field<'a>(line: &'a str, key: &str) -> Result<&'a str, String> {
            line.split(key)
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .ok_or_else(|| format!("missing showinfo {key}"))
        }
        let ordinal = field(line, " n:")?
            .parse::<u64>()
            .map_err(|_| "invalid frame ordinal")?;
        if ordinal != self.next {
            return Err("nonsequential FFmpeg frame ordinal".into());
        }
        let pts = field(line, " pts:")?
            .parse::<i64>()
            .map_err(|_| "invalid integer frame PTS")?;
        if field(line, " fmt:")? != "rgba" {
            return Err("FFmpeg frame is not RGBA".into());
        }
        let (width, height) = field(line, " s:")?
            .split_once('x')
            .ok_or("invalid frame extent")?;
        let width = width.parse().map_err(|_| "invalid frame width")?;
        let height = height.parse().map_err(|_| "invalid frame height")?;
        let byte_len = usize::try_from(self.limits.validate_frame(width, height)?)
            .map_err(|_| "frame byte count overflow")?;
        let time_base = self.time_base.ok_or("frame precedes filter time base")?;
        self.next = self.next.checked_add(1).ok_or("frame ordinal overflow")?;
        Ok(Some(FfmpegFrameMetadata {
            ordinal,
            pts,
            time_base,
            width,
            height,
            byte_len,
        }))
    }
}
pub fn finish_rgba(
    metadata: FfmpegFrameMetadata,
    pixels: Vec<u8>,
) -> Result<Arc<RgbaImage>, String> {
    if pixels.len() != metadata.byte_len {
        return Err("truncated FFmpeg RGBA frame".into());
    }
    RgbaImage::new(metadata.width, metadata.height, pixels).map(Arc::new)
}

#[derive(Clone, Copy)]
struct Demand {
    serial: u64,
    session: VideoSessionKey,
    target: Timestamp,
}
#[derive(Clone, Copy)]
struct Control {
    demand: Option<Demand>,
    shutdown: bool,
}
struct Ready {
    serial: u64,
    event: VideoDecodeEvent,
}
pub struct FfmpegDecoder {
    capabilities: VideoDecoderCapabilities,
    control: watch::Sender<Control>,
    ready: mpsc::Receiver<Ready>,
    owner: Option<JoinHandle<Result<(), String>>>,
    current: Option<Demand>,
    serial: u64,
}
impl FfmpegDecoder {
    /// Preparation-time construction resolves PATH exactly once, without launching a child.
    pub fn open(
        resource: VideoResourceDescriptor,
        transform: VideoTransform,
        config: FfmpegDecoderConfig,
    ) -> Result<Self, String> {
        config.asset_limits.validate()?;
        config.frame_limits.validate()?;
        if config.max_working_bytes == 0 {
            return Err("native decoder working byte budget must be positive".into());
        }
        if config.frame_limits.max_frames < 3 {
            return Err("native decoder needs three bounded frame slots".into());
        }
        let (control, rx) = watch::channel(Control {
            demand: None,
            shutdown: false,
        });
        let (tx, ready) = mpsc::channel(1);
        let executable = resolve_executable(config.executable.as_deref());
        let path = match resource.data {
            VideoResource::File(path) => Some(path),
            VideoResource::Encoded(_) => None,
        };
        let reason = if resource.encoded_bytes > config.asset_limits.max_encoded_file_bytes as u64 {
            Some("encoded video file exceeds limit".into())
        } else if path.is_none() {
            Some("native FFmpeg requires a prepared file resource".into())
        } else if executable.is_none() {
            Some("FFmpeg executable unavailable; set executable override or PATH".into())
        } else {
            None
        };
        let capabilities = VideoDecoderCapabilities {
            available: reason.is_none(),
            reason,
        };
        let owner = if capabilities.available {
            let path = path.unwrap();
            let executable = executable.unwrap();
            let probe = executable
                .parent()
                .map(|parent| {
                    parent.join(if cfg!(windows) {
                        "ffprobe.exe"
                    } else {
                        "ffprobe"
                    })
                })
                .filter(|candidate| candidate.is_file())
                .or_else(|| {
                    resolve_executable(Some(Path::new(if cfg!(windows) {
                        "ffprobe.exe"
                    } else {
                        "ffprobe"
                    })))
                });
            Some(
                std::thread::Builder::new()
                    .name("video-ffmpeg-io".into())
                    .spawn(move || {
                        tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|e| e.to_string())?
                            .block_on(io_owner(executable, probe, path, transform, config, rx, tx))
                    })
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        Ok(Self {
            capabilities,
            control,
            ready,
            owner,
            current: None,
            serial: 0,
        })
    }
    pub fn shutdown(&mut self) {
        self.current = None;
        self.control.send_replace(Control {
            demand: None,
            shutdown: true,
        });
        while self.ready.try_recv().is_ok() {}
    }
    /// Explicit preparation/cleanup owner operation; never call from a callback.
    pub fn join(&mut self) -> Result<(), String> {
        self.shutdown();
        if let Some(owner) = self.owner.take() {
            owner.join().map_err(|_| "FFmpeg IO owner panicked")??;
        }
        Ok(())
    }
}
impl VideoDecoderPort for FfmpegDecoder {
    fn capabilities(&self) -> VideoDecoderCapabilities {
        self.capabilities.clone()
    }
    fn request(&mut self, session: VideoSessionKey, target: Timestamp) -> Result<(), String> {
        if !self.capabilities.available {
            return Err(self.capabilities.reason.clone().unwrap_or_default());
        }
        if self.control.borrow().shutdown {
            return Err("FFmpeg owner is shut down".into());
        }
        if self
            .current
            .is_none_or(|old| old.session != session || target < old.target)
        {
            self.serial = self
                .serial
                .checked_add(1)
                .ok_or("decoder generation overflow")?;
            while self.ready.try_recv().is_ok() {}
        }
        let demand = Demand {
            serial: self.serial,
            session,
            target,
        };
        self.current = Some(demand);
        self.control.send_replace(Control {
            demand: Some(demand),
            shutdown: false,
        });
        Ok(())
    }
    fn try_next(&mut self) -> Option<VideoDecodeEvent> {
        while let Ok(ready) = self.ready.try_recv() {
            if self
                .current
                .is_some_and(|demand| demand.serial == ready.serial)
            {
                return Some(ready.event);
            }
        }
        None
    }
    fn retire(&mut self, session: VideoSessionKey) {
        if self.current.is_some_and(|demand| demand.session == session) {
            self.current = None;
            self.control.send_replace(Control {
                demand: None,
                shutdown: false,
            });
            while self.ready.try_recv().is_ok() {}
        }
    }
}
impl Drop for FfmpegDecoder {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn resolve_executable(override_path: Option<&Path>) -> Option<PathBuf> {
    let name = override_path.map(Path::to_path_buf).unwrap_or_else(|| {
        PathBuf::from(if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        })
    });
    if name.is_absolute() || name.components().count() > 1 {
        return name.is_file().then_some(name);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|path| path.join(&name))
            .find(|path| path.is_file())
    })
}

async fn io_owner(
    executable: PathBuf,
    probe: Option<PathBuf>,
    path: PathBuf,
    transform: VideoTransform,
    config: FfmpegDecoderConfig,
    mut control: watch::Receiver<Control>,
    ready: mpsc::Sender<Ready>,
) -> Result<(), String> {
    let mut completed = None;
    let mut origin: Option<(i64, VideoTimeBase)> = None;
    let keyframes = if let Some(probe) = probe {
        probe_keyframes(&probe, &path, &mut control)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    loop {
        let state = *control.borrow_and_update();
        if state.shutdown {
            return Ok(());
        }
        let Some(demand) = state.demand.filter(|d| Some(d.serial) != completed) else {
            if control.changed().await.is_err() {
                return Ok(());
            }
            continue;
        };
        let mut command = Command::new(&executable);
        command.args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "info",
            "-threads",
            "1",
            "-filter_threads",
            "1",
            "-copyts",
        ]);
        if let Some((ticks, base)) = origin {
            let absolute = base
                .timestamp(ticks, 0)?
                .as_nanos()
                .checked_add(demand.target.as_nanos())
                .ok_or("seek timestamp overflow")?;
            if let Some(anchor) = keyframes
                .iter()
                .copied()
                .take_while(|anchor| *anchor <= absolute)
                .last()
                .filter(|anchor| *anchor > 0)
            {
                command
                    .args(["-seek_timestamp", "1", "-ss"])
                    .arg(decimal_seconds(anchor))
                    // Keep the demuxer's keyframe preroll. FFmpeg's accurate
                    // seek trimming can offset the cut again for nonzero
                    // stream starts despite -seek_timestamp/-copyts, yielding
                    // an empty stream. Our integer-PTS selector performs the
                    // exact cut and retains the latest pretarget frame.
                    .arg("-noaccurate_seek");
            }
        }
        command
            .arg("-i")
            .arg(&path)
            .args([
                "-map",
                "0:v:0",
                "-an",
                "-sn",
                "-dn",
                "-vf",
                "format=rgba,showinfo",
                "-fps_mode",
                "passthrough",
                "-pix_fmt",
                "rgba",
                "-threads",
                "1",
                "-f",
                "rawvideo",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                if !publish(
                    &ready,
                    &mut control,
                    demand,
                    VideoDecodeEvent::Failed {
                        session: demand.session,
                        reason: format!("FFmpeg launch: {error}"),
                    },
                )
                .await
                {
                    continue;
                }
                completed = Some(demand.serial);
                continue;
            }
        };
        let stdout = child.stdout.take().ok_or("missing FFmpeg stdout")?;
        let stderr = child.stderr.take().ok_or("missing FFmpeg stderr")?;
        let (metadata_tx, metadata_rx) = mpsc::channel(2);
        let limits = config.asset_limits;
        let mut stderr_task = tokio::spawn(async move {
            let mut parser = ShowInfoParser::new(limits);
            let mut reader = BufReader::new(stderr);
            loop {
                // A bounded line buffer prevents hostile diagnostics growing memory.
                let mut line = Vec::new();
                loop {
                    let available = reader.fill_buf().await.map_err(|e| e.to_string())?;
                    if available.is_empty() {
                        break;
                    }
                    let count = available
                        .iter()
                        .position(|byte| *byte == b'\n')
                        .map_or(available.len(), |n| n + 1);
                    if line.len() + count > 16 * 1024 {
                        return Err("FFmpeg diagnostic line exceeds limit".into());
                    }
                    line.extend_from_slice(&available[..count]);
                    let ended = line.last() == Some(&b'\n');
                    reader.consume(count);
                    if ended {
                        break;
                    }
                }
                if line.is_empty() {
                    return Ok::<(), String>(());
                }
                let line =
                    std::str::from_utf8(&line).map_err(|_| "invalid FFmpeg diagnostic text")?;
                if let Some(metadata) = parser.parse_line(line)? {
                    if metadata_tx.send(metadata).await.is_err() {
                        return Ok(());
                    }
                }
            }
        });
        let mut parser_finished = false;
        let mut parser_error = None;
        let outcome = {
            let mut stream_control = control.clone();
            let stream = decode_stream(
                stdout,
                metadata_rx,
                &ready,
                &mut stream_control,
                demand,
                transform,
                &config,
                &mut origin,
            );
            tokio::pin!(stream);
            loop {
                tokio::select! {
                    result = &mut stream => break Some(result),
                    parsed = &mut stderr_task, if !parser_finished => {
                        parser_finished = true;
                        match parsed {
                            Ok(Ok(())) => {},
                            Ok(Err(reason)) => { parser_error = Some(reason.clone()); break Some(Err(reason)); },
                            Err(error) => { let reason = error.to_string(); parser_error = Some(reason.clone()); break Some(Err(reason)); }
                        }
                    },
                    changed = control.changed() => {
                        if changed.is_err() || control.borrow().shutdown || control.borrow().demand.is_none_or(|new| new.serial != demand.serial) { break None; }
                    }
                }
            }
        };
        // Cancel the readers before waiting; neither can remain blocked on credits.
        if outcome.as_ref().is_none_or(|result| result.is_err()) {
            if !parser_finished {
                stderr_task.abort();
                let _ = stderr_task.await;
            }
            let _ = child.start_kill();
        } else if !parser_finished {
            match stderr_task.await {
                Ok(Ok(())) => {}
                Ok(Err(reason)) => parser_error = Some(reason),
                Err(error) => parser_error = Some(error.to_string()),
            }
        }
        let status = loop {
            tokio::select! {
                status = child.wait() => break status.map_err(|e| e.to_string())?,
                changed = control.changed() => {
                    if changed.is_err() || control.borrow().shutdown || control.borrow().demand.is_none_or(|new| new.serial != demand.serial) {
                        let _ = child.start_kill();
                    }
                }
            }
        };
        if let Some(result) = outcome {
            completed = Some(demand.serial);
            let result = result.and_then(|end| {
                if let Some(error) = parser_error {
                    return Err(error);
                }
                if !status.success() {
                    return Err(format!("FFmpeg exited {status}"));
                }
                Ok(end)
            });
            let event = match result {
                Ok(end) => VideoDecodeEvent::End {
                    session: demand.session,
                    end,
                },
                Err(reason) => VideoDecodeEvent::Failed {
                    session: demand.session,
                    reason,
                },
            };
            publish(&ready, &mut control, demand, event).await;
        }
    }
}
async fn publish(
    ready: &mpsc::Sender<Ready>,
    control: &mut watch::Receiver<Control>,
    demand: Demand,
    event: VideoDecodeEvent,
) -> bool {
    let send = ready.send(Ready {
        serial: demand.serial,
        event,
    });
    tokio::pin!(send);
    loop {
        tokio::select! {
            result = &mut send => return result.is_ok(),
            changed = control.changed() => {
                if changed.is_err() || control.borrow().shutdown || control.borrow().demand.is_none_or(|new| new.serial != demand.serial) { return false; }
            }
        }
    }
}
async fn emit(
    ready: &mpsc::Sender<Ready>,
    demand: Demand,
    frame: VideoFrame,
) -> Result<(), String> {
    let pts = frame.pts;
    ready
        .send(Ready {
            serial: demand.serial,
            event: VideoDecodeEvent::Frame(frame),
        })
        .await
        .map_err(|_| "frame consumer closed".to_owned())?;
    ready
        .send(Ready {
            serial: demand.serial,
            event: VideoDecodeEvent::Watermark {
                session: demand.session,
                through: pts,
            },
        })
        .await
        .map_err(|_| "frame consumer closed".to_owned())
}
async fn decode_stream(
    mut stdout: tokio::process::ChildStdout,
    mut metadata: mpsc::Receiver<FfmpegFrameMetadata>,
    ready: &mpsc::Sender<Ready>,
    control: &mut watch::Receiver<Control>,
    demand: Demand,
    transform: VideoTransform,
    config: &FfmpegDecoderConfig,
    origin: &mut Option<(i64, VideoTimeBase)>,
) -> Result<Option<Timestamp>, String> {
    let mut held: Option<VideoFrame> = None;
    let mut last = None;
    let mut revision = 0u64;
    while let Some(header) = metadata.recv().await {
        let (source_origin, base) = *origin.get_or_insert((header.pts, header.time_base));
        if header.time_base != base {
            return Err("source filter time base changed across seek".into());
        }
        let pts = base.timestamp(header.pts, source_origin)?;
        if last.is_some_and(|previous| pts <= previous) {
            return Err("FFmpeg presentation points are not strictly ordered".into());
        }
        if header.byte_len as u64 > config.max_working_bytes / 4 {
            return Err("native frame working budget exceeded".into());
        }
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(header.byte_len)
            .map_err(|_| "RGBA frame allocation refused")?;
        pixels.resize(header.byte_len, 0);
        stdout
            .read_exact(&mut pixels)
            .await
            .map_err(|_| "truncated FFmpeg RGBA frame")?;
        let raw = finish_rgba(header, pixels)?;
        // Ready + preroll + source + all transform variants fit the total cap,
        // including peak temporary ownership during crop/key preparation.
        let transform_limits = VideoAssetLimits {
            max_frame_bytes: config
                .asset_limits
                .max_frame_bytes
                .min(config.max_working_bytes / 4),
            ..config.asset_limits
        };
        let variants = transform.apply(&raw, transform_limits)?;
        if variants.retained_bytes > config.max_working_bytes / 4 {
            return Err("native transformed frame working budget exceeded".into());
        }
        let image = Arc::clone(
            variants
                .get(demand.session.channel)
                .ok_or("keyed layer variant unavailable")?,
        );
        if image.byte_len() > config.frame_limits.max_frame_bytes
            || image.byte_len() > config.frame_limits.max_bytes / 3
        {
            return Err("native retained frame budget exceeded".into());
        }
        drop(variants);
        drop(raw);
        revision = revision.checked_add(1).ok_or("frame revision overflow")?;
        let frame = VideoFrame {
            session: demand.session,
            pts,
            revision,
            image,
        };
        last = Some(pts);
        let target = control
            .borrow()
            .demand
            .map_or(demand.target, |new| new.target);
        if pts <= target {
            held = Some(frame);
            continue;
        }
        if let Some(previous) = held.take() {
            emit(ready, demand, previous).await?;
        }
        emit(ready, demand, frame).await?;
        // The future point proves the predecessor complete. Await song demand,
        // keeping pipe/metadata pressure bounded while the owner watches cancel.
        loop {
            let target = control
                .borrow_and_update()
                .demand
                .map_or(demand.target, |new| new.target);
            if target >= pts {
                break;
            }
            control
                .changed()
                .await
                .map_err(|_| "decoder control closed")?;
        }
    }
    if let Some(previous) = held.take() {
        emit(ready, demand, previous).await?;
    }
    let mut extra = [0u8; 1];
    if stdout.read(&mut extra).await.map_err(|e| e.to_string())? != 0 {
        return Err("RGBA bytes without frame metadata".into());
    }
    Ok(last)
}

fn decimal_seconds(nanos: i64) -> String {
    let value = i128::from(nanos);
    let sign = if value < 0 { "-" } else { "" };
    let magnitude = value.abs();
    format!(
        "{sign}{}.{:09}",
        magnitude / 1_000_000_000,
        magnitude % 1_000_000_000
    )
}
/// Optional, bounded random-access index. Missing/unsupported probe falls back
/// to decoding from the beginning; actual filter origin remains authoritative.
async fn probe_keyframes(
    executable: &Path,
    path: &Path,
    control: &mut watch::Receiver<Control>,
) -> Result<Vec<i64>, String> {
    let mut child = Command::new(executable)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-skip_frame",
            "nokey",
            "-show_frames",
            "-show_streams",
            "-show_entries",
            "frame=best_effort_timestamp:stream=time_base",
            "-of",
            "compact=p=1:nk=0",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut output = Vec::new();
    let stdout = child.stdout.take().ok_or("missing ffprobe stdout")?;
    let mut limited = stdout.take(2 * 1024 * 1024 + 1);
    let result = {
        // EOF on stdout does not imply process exit. Keep both operations in
        // the same deadline/cancellation scope so cleanup can always reap the
        // probe, including a wrapper that closes its pipe before hanging.
        let probe = async {
            limited
                .read_to_end(&mut output)
                .await
                .map_err(|e| e.to_string())?;
            if output.len() > 2 * 1024 * 1024 {
                return Err("keyframe index exceeds limit".into());
            }
            child.wait().await.map_err(|e| e.to_string())
        };
        tokio::pin!(probe);
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(5));
        tokio::pin!(timeout);
        loop {
            if control.borrow().shutdown {
                break Err("keyframe index cancelled".into());
            }
            tokio::select! {
                result = &mut probe => break result,
                _ = &mut timeout => break Err("keyframe index timed out".into()),
                changed = control.changed() => {
                    if changed.is_err() || control.borrow().shutdown { break Err("keyframe index cancelled".into()); }
                }
            }
        }
    };
    if result.is_err() {
        let _ = child.start_kill();
        // Reap after kill, outside the cancelled probe future. No child or
        // pipe-reader task is transferred to a detached cleanup owner.
        let _ = child.wait().await;
    }
    let status = result?;
    if !status.success() {
        return Err("keyframe index unavailable or exceeds limit".into());
    }
    let text = std::str::from_utf8(&output).map_err(|_| "invalid ffprobe index")?;
    let mut base = None;
    let mut ticks = Vec::new();
    for line in text.lines() {
        for field in line.split('|') {
            if let Some(ratio) = field.strip_prefix("time_base=") {
                let (n, d) = ratio.split_once('/').ok_or("invalid probe time base")?;
                base = Some(VideoTimeBase::new(
                    n.parse().map_err(|_| "invalid probe numerator")?,
                    d.parse().map_err(|_| "invalid probe denominator")?,
                )?);
            }
            if let Some(value) = field.strip_prefix("best_effort_timestamp=") {
                if ticks.len() >= 16384 {
                    return Err("keyframe index capacity exceeded".into());
                }
                ticks.push(
                    value
                        .parse::<i64>()
                        .map_err(|_| "invalid probe frame PTS")?,
                );
            }
        }
    }
    let base = base.ok_or("probe lacks video time base")?;
    let mut points = ticks
        .into_iter()
        .map(|pts| base.timestamp(pts, 0).map(|point| point.as_nanos()))
        .collect::<Result<Vec<_>, _>>()?;
    points.sort_unstable();
    points.dedup();
    Ok(points)
}
