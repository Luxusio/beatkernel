//! Renderer-owned movie selection; the dedicated codec Worker supplies pixels.
use crate::{
    player_chart::PlayerChart,
    poor_background::BgaPresentation,
    texture::RgbaImage,
    video::{VideoFrame, VideoFrameAdmission, VideoFrameLimits, VideoFrameQueue, VideoSessionKey},
    video_assets::{VideoAssetLimits, VideoAssets, VideoDescriptor, VideoTransform},
};
use beatkernel::time::Timestamp;
use beatkernel_bms::{BgaChannel, BgaCrop, ImageId, PoorBgaMode};
use std::{collections::BTreeMap, sync::Arc};
use wasm_bindgen::prelude::*;

fn error(value: impl ToString) -> JsValue {
    JsValue::from_str(&value.to_string())
}
fn get(value: &JsValue, key: &str) -> Result<JsValue, JsValue> {
    js_sys::Reflect::get(value, &key.into())
}
fn set(value: &js_sys::Object, key: &str, field: JsValue) -> Result<(), JsValue> {
    js_sys::Reflect::set(value, &key.into(), &field).map(|_| ())
}
fn integer(value: JsValue, min: f64, max: f64) -> Result<f64, JsValue> {
    value
        .as_f64()
        .filter(|n| n.is_finite() && n.fract() == 0.0 && *n >= min && *n <= max)
        .ok_or_else(|| error("invalid video integer"))
}
fn array(value: JsValue, length: usize) -> Result<js_sys::Array, JsValue> {
    if !js_sys::Array::is_array(&value) {
        return Err(error("invalid video array"));
    }
    let values = js_sys::Array::from(&value);
    if values.length() as usize != length {
        return Err(error("invalid video array extent"));
    }
    Ok(values)
}
pub(crate) fn transform_from_js(value: &JsValue) -> Result<VideoTransform, JsValue> {
    let crop = get(value, "crop")?;
    let crop = if crop.is_null() {
        None
    } else {
        let values = array(crop, 7)?;
        let source = integer(values.get(0), 0.0, 3843.0)? as u16;
        let mut coords = [0i32; 6];
        for (index, coord) in coords.iter_mut().enumerate() {
            *coord = integer(
                values.get(index as u32 + 1),
                i32::MIN as f64,
                i32::MAX as f64,
            )? as i32;
        }
        let crop = BgaCrop {
            source: ImageId(source),
            source_rect: coords[..4].try_into().unwrap(),
            destination: coords[4..].try_into().unwrap(),
        };
        crop.validate().map_err(error)?;
        Some(crop)
    };
    let canvas = get(value, "canvas")?;
    let canvas = if canvas.is_null() {
        None
    } else {
        let values = array(canvas, 2)?;
        let extent = [
            integer(values.get(0), 1.0, 16384.0)? as u32,
            integer(values.get(1), 1.0, 16384.0)? as u32,
        ];
        VideoAssetLimits::default()
            .validate_frame(extent[0], extent[1])
            .map_err(error)?;
        Some(extent)
    };
    if crop.is_some() && canvas.is_none() {
        return Err(error("video crop requires canvas"));
    }
    let keyed = get(value, "keyed")?
        .as_bool()
        .ok_or_else(|| error("invalid video key flag"))?;
    Ok(VideoTransform {
        crop,
        canvas,
        keyed,
    })
}
fn transform_to_js(transform: VideoTransform) -> Result<JsValue, JsValue> {
    let object = js_sys::Object::new();
    let crop = transform
        .crop
        .map(|crop| {
            let values = js_sys::Array::new();
            values.push(&JsValue::from(crop.source.0));
            for n in crop.source_rect.into_iter().chain(crop.destination) {
                values.push(&JsValue::from(n));
            }
            values.into()
        })
        .unwrap_or(JsValue::NULL);
    let canvas = transform
        .canvas
        .map(|extent| {
            let values = js_sys::Array::new();
            for n in extent {
                values.push(&JsValue::from(n));
            }
            values.into()
        })
        .unwrap_or(JsValue::NULL);
    set(&object, "crop", crop)?;
    set(&object, "canvas", canvas)?;
    set(&object, "keyed", transform.keyed.into())?;
    Ok(object.into())
}
pub(crate) fn registration(assets: &VideoAssets) -> Result<JsValue, JsValue> {
    let transfer = assets.export_video().map_err(error)?;
    let resources = js_sys::Array::new();
    for bytes in transfer.resources {
        resources.push(&js_sys::Uint8Array::from(bytes.as_ref()));
    }
    let images = js_sys::Array::new();
    for (image, descriptor) in transfer.images {
        let value = js_sys::Object::new();
        set(&value, "image", image.0.into())?;
        set(
            &value,
            "resource",
            JsValue::from(descriptor.resource as u32),
        )?;
        set(&value, "transform", transform_to_js(descriptor.transform)?)?;
        images.push(&value);
    }
    let result = js_sys::Object::new();
    set(&result, "resources", resources.into())?;
    set(&result, "images", images.into())?;
    Ok(result.into())
}

struct Session {
    completed: Option<Timestamp>,
    key: VideoSessionKey,
    target: Timestamp,
    queue: VideoFrameQueue,
    descriptor: VideoDescriptor,
}
#[derive(Default)]
pub(crate) struct BrowserVideo {
    content: u64,
    serial: u64,
    images: BTreeMap<ImageId, VideoDescriptor>,
    sessions: [Option<Session>; 16],
}
impl BrowserVideo {
    pub(crate) fn register(&mut self, content: u64, images: &js_sys::Array) -> Result<(), JsValue> {
        if content == 0 || content > 9_007_199_254_740_991 || images.length() > 3844 {
            return Err(error("video registry identity or extent invalid"));
        }
        let mut registry = BTreeMap::new();
        for value in images.iter() {
            let image = ImageId(integer(get(&value, "image")?, 0.0, 3843.0)? as u16);
            let descriptor = VideoDescriptor {
                resource: integer(get(&value, "resource")?, 0.0, 3843.0)? as usize,
                transform: transform_from_js(&get(&value, "transform")?)?,
            };
            if registry.insert(image, descriptor).is_some() {
                return Err(error("duplicate video image"));
            }
        }
        self.retire();
        self.content = content;
        self.images = registry;
        Ok(())
    }
    pub(crate) fn retire(&mut self) {
        self.sessions = std::array::from_fn(|_| None);
        self.images.clear();
        self.content = 0;
    }
    pub(crate) fn begin(&self) -> [bool; 16] {
        [false; 16]
    }
    pub(crate) fn prepare(
        &mut self,
        view: usize,
        chart: &PlayerChart,
        now: Timestamp,
        last_miss: Option<Timestamp>,
        presentation: BgaPresentation,
        wanted: &mut [bool; 16],
    ) -> Result<(), JsValue> {
        let activations = chart.bga_activations(now);
        let poor_replaces = chart.poor_bga_mode == PoorBgaMode::Replace
            && last_miss.is_some_and(|miss| {
                let age = i128::from(now.as_nanos()) - i128::from(miss.as_nanos());
                age >= 0 && age < 500_000_000
            });
        for (channel, image) in [
            presentation.state.base,
            presentation.state.layer,
            presentation.state.layer2,
            presentation.poor_overlay,
        ]
        .into_iter()
        .enumerate()
        {
            let Some(image) = image else {
                continue;
            };
            let Some(descriptor) = self.images.get(&image).copied() else {
                continue;
            };
            let index = match channel {
                0 if poor_replaces => 2,
                0 => 0,
                1 => 1,
                2 => 3,
                _ => 2,
            };
            let Some(activation) = activations[index].filter(|a| a.image == image) else {
                continue;
            };
            let Some(mut key) =
                VideoSessionKey::from_activation(self.content, 0, activation, last_miss)
            else {
                continue;
            };
            let target = key.target(now).map_err(error)?;
            if target.as_nanos() < 0 {
                continue;
            }
            let slot = view * 4 + channel;
            let same = self.sessions[slot].as_ref().is_some_and(|old| {
                key.generation = old.key.generation;
                key == old.key && target >= old.target
            });
            if !same {
                self.serial = self
                    .serial
                    .checked_add(1)
                    .filter(|n| *n <= 9_007_199_254_740_991)
                    .ok_or_else(|| error("video session identity exhausted"))?;
                key.generation = self.serial;
                let queue =
                    VideoFrameQueue::new(key, VideoFrameLimits::default()).map_err(error)?;
                self.sessions[slot] = Some(Session {
                    completed: None,
                    key,
                    target,
                    queue,
                    descriptor,
                });
            }
            let session = self.sessions[slot].as_mut().unwrap();
            session.target = target;
            session.queue.select(target);
            wanted[slot] = true;
        }
        Ok(())
    }
    pub(crate) fn demands(&mut self, wanted: [bool; 16]) -> Result<js_sys::Array, JsValue> {
        let result = js_sys::Array::new();
        for (slot, wanted) in wanted.into_iter().enumerate() {
            if !wanted {
                self.sessions[slot] = None;
                continue;
            }
            let session = self.sessions[slot].as_ref().unwrap();
            let value = js_sys::Object::new();
            for (name, n) in [
                ("slot", slot as u64),
                ("content", self.content),
                ("generation", session.key.generation),
                ("resource", session.descriptor.resource as u64),
            ] {
                set(&value, name, JsValue::from_f64(n as f64))?;
            }
            set(
                &value,
                "targetNs",
                js_sys::BigInt::from(session.target.as_nanos()).into(),
            )?;
            let mut transform = session.descriptor.transform;
            // Channel intent determines the exact-black variant; aliases retain raw Base/Poor.
            transform.keyed = matches!(session.key.channel, BgaChannel::Layer | BgaChannel::Layer2);
            let transform =
                if transform.crop.is_none() && transform.canvas.is_none() && !transform.keyed {
                    JsValue::NULL
                } else {
                    transform_to_js(transform)?
                };
            set(&value, "transform", transform)?;
            result.push(&value);
        }
        Ok(result)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn admit(
        &mut self,
        slot: usize,
        generation: u64,
        revision: u64,
        pts: i64,
        width: u32,
        height: u32,
        bytes: &js_sys::Uint8Array,
    ) -> Result<bool, JsValue> {
        self.admit_with_completion(slot, generation, revision, pts, width, height, bytes, None)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn admit_with_completion(
        &mut self,
        slot: usize,
        generation: u64,
        revision: u64,
        pts: i64,
        width: u32,
        height: u32,
        bytes: &js_sys::Uint8Array,
        through: Option<i64>,
    ) -> Result<bool, JsValue> {
        let Some(session) = self
            .sessions
            .get(slot)
            .and_then(Option::as_ref)
            .filter(|s| s.key.generation == generation)
        else {
            // The renderer consumes obsolete transferred ownership too.
            return Ok(true);
        };
        let size = VideoAssetLimits::default()
            .validate_frame(width, height)
            .map_err(error)?;
        if size != u64::from(bytes.length()) {
            return Err(error("video RGBA extent differs"));
        }
        let total: u64 = self
            .sessions
            .iter()
            .flatten()
            .map(|s| s.queue.buffered_bytes())
            .sum();
        let predicted = if let Some(through) = through {
            session.queue.completed_admission_bytes(
                Timestamp::from_nanos(pts),
                revision,
                size,
                session.target,
                Timestamp::from_nanos(through),
            )
        } else {
            session
                .queue
                .admission_bytes(Timestamp::from_nanos(pts), revision, size)
        }
        .map_err(error)?;
        let Some(predicted) = predicted else {
            return Ok(false);
        };
        if total
            .checked_sub(session.queue.buffered_bytes())
            .and_then(|bytes| bytes.checked_add(predicted))
            .is_none_or(|n| n > VideoFrameLimits::default().max_bytes)
        {
            return Ok(false);
        }
        let frame = VideoFrame {
            session: session.key,
            pts: Timestamp::from_nanos(pts),
            revision,
            image: Arc::new(RgbaImage::new(width, height, bytes.to_vec()).map_err(error)?),
        };
        let session = self.sessions[slot].as_mut().unwrap();
        let admission = if let Some(through) = through {
            let through = Timestamp::from_nanos(through);
            let admission = session
                .queue
                .push_completed(frame, session.target, through)
                .map_err(error)?;
            if !matches!(admission, VideoFrameAdmission::Backpressure(_)) {
                session.completed = session.queue.completed_through();
            }
            admission
        } else {
            session.queue.push(frame).map_err(error)?
        };
        Ok(matches!(
            admission,
            VideoFrameAdmission::Accepted
                | VideoFrameAdmission::Stale
                | VideoFrameAdmission::Obsolete
        ))
    }
    pub(crate) fn watermark(
        &mut self,
        slot: usize,
        generation: u64,
        through: i64,
    ) -> Result<bool, JsValue> {
        let Some(session) = self
            .sessions
            .get_mut(slot)
            .and_then(Option::as_mut)
            .filter(|s| s.key.generation == generation)
        else {
            return Ok(false);
        };
        let through = Timestamp::from_nanos(through);
        // A tighter credit window can finish less lookahead than an earlier
        // request; those presentation points have already completed.
        if session
            .completed
            .is_some_and(|completed| through <= completed)
        {
            return Ok(true);
        }
        session
            .queue
            .watermark(session.key, through)
            .map_err(error)?;
        session.completed = Some(through);
        Ok(true)
    }
    pub(crate) fn end(&mut self, slot: usize, generation: u64, end: i64) -> Result<bool, JsValue> {
        let Some(session) = self
            .sessions
            .get_mut(slot)
            .and_then(Option::as_mut)
            .filter(|s| s.key.generation == generation)
        else {
            return Ok(false);
        };
        session
            .queue
            .finish(Some(Timestamp::from_nanos(end)))
            .map_err(error)?;
        Ok(true)
    }
    pub(crate) fn selected(&mut self) -> [Option<&VideoFrame>; 16] {
        let mut result = [None; 16];
        for (slot, session) in self.sessions.iter_mut().enumerate() {
            if let Some(session) = session {
                result[slot] = session.queue.select(session.target);
            }
        }
        result
    }
    pub(crate) fn active(&self) -> [bool; 16] {
        self.sessions.each_ref().map(Option::is_some)
    }
}

/// Pixel transforms execute only in the dedicated decode Worker WASM instance.
#[wasm_bindgen]
pub fn transform_video_frame(
    width: u32,
    height: u32,
    rgba: js_sys::Uint8Array,
    transform: JsValue,
) -> Result<JsValue, JsValue> {
    let limits = VideoAssetLimits::default();
    let size = limits.validate_frame(width, height).map_err(error)?;
    if size != u64::from(rgba.length()) {
        return Err(error("video transform RGBA extent differs"));
    }
    let transform = transform_from_js(&transform)?;
    let source = Arc::new(RgbaImage::new(width, height, rgba.to_vec()).map_err(error)?);
    let variants = transform.apply(&source, limits).map_err(error)?;
    let image = variants
        .get(if transform.keyed {
            BgaChannel::Layer
        } else {
            BgaChannel::Base
        })
        .ok_or_else(|| error("video transform variant unavailable"))?;
    let result = js_sys::Object::new();
    set(&result, "width", image.width().into())?;
    set(&result, "height", image.height().into())?;
    set(
        &result,
        "rgba",
        js_sys::Uint8Array::from(image.pixels()).into(),
    )?;
    Ok(result.into())
}
