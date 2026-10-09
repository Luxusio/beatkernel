//! Pure original-song movie selection. Adapters own source IO and decoding.
use std::sync::Arc;

use beatkernel::time::Timestamp;
use beatkernel_bms::{BgaChannel, ImageId};

use crate::{bga::BgaActivation, texture::RgbaImage};

/// Seconds per source PTS tick, with an explicit stable source origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoTimeBase {
    numerator: u32,
    denominator: u32,
}

impl VideoTimeBase {
    pub fn new(numerator: u32, denominator: u32) -> Result<Self, String> {
        if numerator == 0 || denominator == 0 {
            return Err("video time base must be positive".into());
        }
        Ok(Self {
            numerator,
            denominator,
        })
    }

    /// One final floor, including negative preroll. Neither PTS subtraction
    /// nor intermediate multiplication is restricted to signed 64 bits.
    pub fn timestamp(self, pts: i64, origin: i64) -> Result<Timestamp, String> {
        let delta = i128::from(pts) - i128::from(origin);
        let nanos = delta * i128::from(self.numerator) * 1_000_000_000;
        let nanos = nanos.div_euclid(i128::from(self.denominator));
        i64::try_from(nanos)
            .map(Timestamp::from_nanos)
            .map_err(|_| "video presentation timestamp overflow".into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoSessionKey {
    pub content: u64,
    pub generation: u64,
    pub image: ImageId,
    pub channel: BgaChannel,
    pub activated_at: Timestamp,
    pub ordinal: Option<u64>,
}

impl VideoSessionKey {
    /// Poor starts only at an accepted miss; its initial BMP00 identity alone
    /// never starts a movie. Other channels restart at each exact marker.
    pub fn from_activation(
        content: u64,
        generation: u64,
        activation: BgaActivation,
        last_miss: Option<Timestamp>,
    ) -> Option<Self> {
        let activated_at = if activation.channel == BgaChannel::Poor {
            last_miss?
        } else {
            activation.activated_at
        };
        Some(Self {
            content,
            generation,
            image: activation.image,
            channel: activation.channel,
            activated_at,
            ordinal: activation.ordinal,
        })
    }

    pub fn target(self, song_time: Timestamp) -> Result<Timestamp, String> {
        song_time
            .as_nanos()
            .checked_sub(self.activated_at.as_nanos())
            .map(Timestamp::from_nanos)
            .ok_or_else(|| "video song target overflow".into())
    }
}

/// Ownership of pixels travels with the exact session and presentation point.
pub struct VideoFrame {
    pub session: VideoSessionKey,
    pub pts: Timestamp,
    pub revision: u64,
    pub image: Arc<RgbaImage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoFrameLimits {
    pub max_frames: usize,
    pub max_bytes: u64,
    pub max_frame_bytes: u64,
}

impl Default for VideoFrameLimits {
    fn default() -> Self {
        Self {
            max_frames: 3,
            max_bytes: 192 * 1024 * 1024,
            max_frame_bytes: 64 * 1024 * 1024,
        }
    }
}

impl VideoFrameLimits {
    pub fn validate(self) -> Result<(), String> {
        if self.max_frames == 0
            || self.max_bytes == 0
            || self.max_frame_bytes == 0
            || self.max_frame_bytes > self.max_bytes
        {
            return Err("video frame limits must be positive and consistent".into());
        }
        Ok(())
    }
}

pub enum VideoFrameAdmission {
    Accepted,
    /// A different session or an already admitted equal/newer revision.
    Stale,
    /// A completed older point than the currently retained selection.
    Obsolete,
    /// The caller still owns this frame and may retry after selection advances.
    Backpressure(VideoFrame),
}

/// Reuses one preallocated sorted vector, including the selected frame in its
/// count and byte budget. Seek backwards requires reset with a new generation.
pub struct VideoFrameQueue {
    session: VideoSessionKey,
    limits: VideoFrameLimits,
    frames: Vec<VideoFrame>,
    bytes: u64,
    watermark: Option<Timestamp>,
    ended: bool,
    selected: Option<Timestamp>,
}

impl VideoFrameQueue {
    pub fn new(session: VideoSessionKey, limits: VideoFrameLimits) -> Result<Self, String> {
        limits.validate()?;
        let mut frames = Vec::new();
        frames
            .try_reserve_exact(limits.max_frames)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            session,
            limits,
            frames,
            bytes: 0,
            watermark: None,
            ended: false,
            selected: None,
        })
    }

    /// Predicts retained bytes before an adapter copies transferred pixels.
    /// None is backpressure; obsolete/replayed revisions retain the current
    /// extent and can be acknowledged without charging phantom extra bytes.
    pub fn admission_bytes(
        &self,
        pts: Timestamp,
        revision: u64,
        size: u64,
    ) -> Result<Option<u64>, String> {
        if size > self.limits.max_frame_bytes {
            return Err("video frame exceeds per-frame byte limit".into());
        }
        if self.ended {
            return Err("video frame arrived after end of stream".into());
        }
        if let Ok(index) = self.frames.binary_search_by_key(&pts, |stored| stored.pts) {
            let stored = &self.frames[index];
            if revision <= stored.revision {
                return Ok(Some(self.bytes));
            }
            let bytes = self.bytes - stored.image.byte_len();
            return Ok((size <= self.limits.max_bytes - bytes).then_some(bytes + size));
        }
        if self.selected.is_some_and(|selected| pts < selected) {
            return Ok(Some(self.bytes));
        }
        Ok((self.frames.len() < self.limits.max_frames
            && size <= self.limits.max_bytes - self.bytes)
            .then_some(self.bytes + size))
    }

    /// Completed pixels may atomically supersede an older eligible selection.
    /// The adapter must supply a real decoder completion watermark, not arrival
    /// time. Future frames and later known eligible frames remain owned.
    pub fn completed_admission_bytes(
        &self,
        pts: Timestamp,
        revision: u64,
        size: u64,
        target: Timestamp,
        through: Timestamp,
    ) -> Result<Option<u64>, String> {
        if size > self.limits.max_frame_bytes {
            return Err("video frame exceeds per-frame byte limit".into());
        }
        if self.ended {
            return Err("video frame arrived after end of stream".into());
        }
        let completed = self.watermark.map_or(through, |old| old.max(through));
        if pts > target || pts > completed {
            return self.admission_bytes(pts, revision, size);
        }
        if self
            .frames
            .iter()
            .any(|frame| frame.pts > pts && frame.pts <= target && frame.pts <= completed)
        {
            return Ok(Some(self.bytes));
        }
        if self
            .frames
            .iter()
            .any(|frame| frame.pts == pts && frame.revision >= revision)
        {
            return Ok(Some(self.bytes));
        }
        let (count, bytes) = self
            .frames
            .iter()
            .filter(|frame| frame.pts > pts)
            .fold((0usize, 0u64), |(count, bytes), frame| {
                (count + 1, bytes + frame.image.byte_len())
            });
        Ok(
            (count < self.limits.max_frames && size <= self.limits.max_bytes - bytes)
                .then_some(bytes + size),
        )
    }
    pub fn push_completed(
        &mut self,
        frame: VideoFrame,
        target: Timestamp,
        through: Timestamp,
    ) -> Result<VideoFrameAdmission, String> {
        if frame.session != self.session {
            return Ok(VideoFrameAdmission::Stale);
        }
        if self
            .completed_admission_bytes(
                frame.pts,
                frame.revision,
                frame.image.byte_len(),
                target,
                through,
            )?
            .is_none()
        {
            return Ok(VideoFrameAdmission::Backpressure(frame));
        }
        let completed = self.watermark.map_or(through, |old| old.max(through));
        if frame.pts > target || frame.pts > completed {
            return self.push(frame);
        }
        if let Some(latest) = self
            .frames
            .iter()
            .filter(|stored| {
                stored.pts > frame.pts && stored.pts <= target && stored.pts <= completed
            })
            .map(|stored| stored.pts)
            .max()
        {
            self.watermark(
                self.session,
                self.watermark.map_or(latest, |old| old.max(latest)),
            )?;
            self.select(target);
            return Ok(VideoFrameAdmission::Obsolete);
        }
        if self
            .frames
            .iter()
            .any(|stored| stored.pts == frame.pts && stored.revision >= frame.revision)
        {
            self.watermark(
                self.session,
                self.watermark.map_or(frame.pts, |old| old.max(frame.pts)),
            )?;
            self.select(target);
            return Ok(VideoFrameAdmission::Stale);
        }
        let pts = frame.pts;
        let old = self.frames.partition_point(|stored| stored.pts < pts);
        self.bytes -= self.frames[..old]
            .iter()
            .map(|stored| stored.image.byte_len())
            .sum::<u64>();
        self.frames.drain(..old);
        let admission = self.push(frame)?;
        // Completion proof permits replacement, but other transferred frames
        // may still be held by the adapter. Publish only this known prefix.
        self.watermark(self.session, self.watermark.map_or(pts, |old| old.max(pts)))?;
        self.select(target);
        Ok(admission)
    }
    pub fn completed_through(&self) -> Option<Timestamp> {
        self.watermark
    }

    pub fn push(&mut self, frame: VideoFrame) -> Result<VideoFrameAdmission, String> {
        if frame.session != self.session {
            return Ok(VideoFrameAdmission::Stale);
        }
        let size = frame.image.byte_len();
        if size > self.limits.max_frame_bytes {
            return Err("video frame exceeds per-frame byte limit".into());
        }
        if self.ended {
            return Err("video frame arrived after end of stream".into());
        }
        let position = self
            .frames
            .binary_search_by_key(&frame.pts, |stored| stored.pts);
        if let Ok(index) = position {
            let stored = &self.frames[index];
            if frame.revision <= stored.revision {
                return Ok(VideoFrameAdmission::Stale);
            }
            let bytes = self.bytes - stored.image.byte_len();
            if size > self.limits.max_bytes - bytes {
                return Ok(VideoFrameAdmission::Backpressure(frame));
            }
            self.bytes = bytes + size;
            self.frames[index] = frame;
            return Ok(VideoFrameAdmission::Accepted);
        }
        if self.selected.is_some_and(|selected| frame.pts < selected) {
            return Ok(VideoFrameAdmission::Obsolete);
        }
        if self.frames.len() == self.limits.max_frames || size > self.limits.max_bytes - self.bytes
        {
            return Ok(VideoFrameAdmission::Backpressure(frame));
        }
        let index = position.unwrap_err();
        self.bytes += size;
        self.frames.insert(index, frame);
        Ok(VideoFrameAdmission::Accepted)
    }

    /// Marks all presentation points at or below `through` complete, after the
    /// adapter has delivered their frames. Reordered arrivals alone cannot do so.
    pub fn watermark(
        &mut self,
        session: VideoSessionKey,
        through: Timestamp,
    ) -> Result<bool, String> {
        if session != self.session {
            return Ok(false);
        }
        if self.watermark.is_some_and(|old| through < old) {
            return Err("video presentation watermark moved backwards".into());
        }
        self.watermark = Some(through);
        Ok(true)
    }

    pub fn select(&mut self, target: Timestamp) -> Option<&VideoFrame> {
        let index = self.frames.iter().rposition(|frame| {
            frame.pts <= target
                && (self.ended || self.watermark.is_some_and(|through| frame.pts <= through))
        })?;
        // Remove older preroll only after its replacement is known complete.
        for frame in &self.frames[..index] {
            self.bytes -= frame.image.byte_len();
        }
        self.frames.drain(..index);
        self.selected = Some(self.frames[0].pts);
        self.frames.first()
    }

    /// EOF establishes completion for all retained reordered frames. Last
    /// eligible pixels remain held; `end` is metadata, never a loop trigger.
    pub fn finish(&mut self, end: Option<Timestamp>) -> Result<(), String> {
        if end.is_some_and(|end| self.frames.last().is_some_and(|frame| frame.pts > end)) {
            return Err("video end precedes buffered presentation frame".into());
        }
        self.ended = true;
        Ok(())
    }

    pub fn reset(&mut self, session: VideoSessionKey) {
        self.frames.clear();
        self.session = session;
        self.bytes = 0;
        self.watermark = None;
        self.ended = false;
        self.selected = None;
    }

    pub fn buffered_len(&self) -> usize {
        self.frames.len()
    }
    pub fn buffered_bytes(&self) -> u64 {
        self.bytes
    }
    pub fn storage_capacity(&self) -> usize {
        self.frames.capacity()
    }
    pub fn storage_ptr(&self) -> *const VideoFrame {
        self.frames.as_ptr()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoDecoderCapabilities {
    pub available: bool,
    pub reason: Option<String>,
}

pub enum VideoDecodeEvent {
    Frame(VideoFrame),
    Watermark {
        session: VideoSessionKey,
        through: Timestamp,
    },
    End {
        session: VideoSessionKey,
        end: Option<Timestamp>,
    },
    Failed {
        session: VideoSessionKey,
        reason: String,
    },
}

/// Statically dispatched adapter seam. Every method is nonblocking: request
/// and retire enqueue IO-owner work; try_next polls bounded ready output.
pub trait VideoDecoderPort {
    fn capabilities(&self) -> VideoDecoderCapabilities;
    fn request(&mut self, session: VideoSessionKey, target: Timestamp) -> Result<(), String>;
    fn try_next(&mut self) -> Option<VideoDecodeEvent>;
    fn retire(&mut self, session: VideoSessionKey);
}
