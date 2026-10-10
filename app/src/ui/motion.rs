//! Pure presentation sampling; the caller owns elapsed time and scheduling.

use crate::{
    scene::{Scene, UiComponentId, UiComponentKey, UiTransform, UiTranslation, MAX_UI_COMPONENTS},
    screen_lifecycle::ScreenInstanceId,
    ui::layout::NodeId,
};
use std::time::Duration;

/// Linear integer-pixel movement of one retained UI surface.
/// Compose local geometry first, then apply the sampled translation to its scene.
/// No geometry, reactive tree, transport clock or timer is owned here.
#[derive(Clone, Copy, Debug)]
pub struct TranslationMotion {
    from: UiTranslation,
    to: UiTranslation,
    duration: Duration,
}
impl TranslationMotion {
    pub fn new(from: UiTranslation, to: UiTranslation, duration: Duration) -> Self {
        Self { from, to, duration }
    }

    /// Exact endpoints, clamped completion; zero duration completes immediately.
    /// Intermediate displacement rounds toward the starting offset.
    pub fn sample(self, elapsed: Duration) -> UiTranslation {
        if elapsed >= self.duration {
            return self.to;
        }
        if elapsed.is_zero() {
            return self.from;
        }
        // Duration::MAX has fewer than 95 bits of nanoseconds; the admitted
        // coordinate difference has at most 26 bits. Their product fits i128
        // on both native and WASM, independent of pointer width.
        let elapsed = elapsed.as_nanos() as i128;
        let duration = self.duration.as_nanos() as i128;
        let from = self.from.offset();
        let to = self.to.offset();
        let axis = |index: usize| {
            let delta = i128::from(to[index]) - i128::from(from[index]);
            let displacement = delta
                .checked_mul(elapsed)
                .expect("bounded duration and coordinate product")
                / duration;
            let value = i128::from(from[index])
                .checked_add(displacement)
                .expect("bounded translation sum");
            i32::try_from(value).expect("interpolation within integer endpoints")
        };
        // Interpolation remains inside the admitted endpoint interval.
        UiTranslation::new(axis(0), axis(1)).expect("bounded translation interpolation")
    }
}

#[cfg(test)]
#[path = "motion_fixtures.rs"]
mod fixtures;

#[derive(Clone, Copy, Debug)]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}
#[derive(Clone, Copy, Debug)]
pub struct ComponentMotion {
    from: UiTransform,
    to: UiTransform,
    duration: Duration,
    easing: Easing,
}
impl ComponentMotion {
    pub fn new(from: UiTransform, to: UiTransform, duration: Duration, easing: Easing) -> Self {
        Self {
            from,
            to,
            duration,
            easing,
        }
    }
    pub fn sample(self, elapsed: Duration) -> UiTransform {
        if elapsed >= self.duration {
            return self.to;
        }
        if elapsed.is_zero() {
            return self.from;
        }
        let t = elapsed.as_secs_f64() / self.duration.as_secs_f64();
        let t = match self.easing {
            Easing::Linear => t,
            Easing::EaseIn => t * t,
            Easing::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Easing::EaseInOut if t < 0.5 => 2.0 * t * t,
            Easing::EaseInOut => 1.0 - 2.0 * (1.0 - t) * (1.0 - t),
        };
        let blend = |a: f32, b: f32| (f64::from(a) + (f64::from(b) - f64::from(a)) * t) as f32;
        UiTransform::new(
            std::array::from_fn(|i| blend(self.from.offset()[i], self.to.offset()[i])),
            std::array::from_fn(|i| blend(self.from.scale()[i], self.to.scale()[i])),
            blend(self.from.opacity(), self.to.opacity()),
        )
        .expect("convex bounded transform interpolation")
    }
}
#[derive(Clone, Copy)]
struct Track {
    key: UiComponentKey,
    id: UiComponentId,
    motion: ComponentMotion,
    start: Duration,
}
/// One retained screen owner. Explicit caller time drives bounded stack updates;
/// no timer thread, view rebuilding, callback work or heap allocation occurs on ticks.
#[derive(Clone)]
pub struct MotionScheduler {
    screen: ScreenInstanceId,
    tracks: [Option<Track>; MAX_UI_COMPONENTS],
    capacity: usize,
    last: Duration,
    suspended: Option<Duration>,
    disposed: bool,
}
impl MotionScheduler {
    pub fn new(screen: ScreenInstanceId, capacity: usize) -> Result<Self, String> {
        if screen.0 == 0 || !(1..=MAX_UI_COMPONENTS).contains(&capacity) {
            return Err("invalid motion owner/capacity".into());
        }
        Ok(Self {
            screen,
            tracks: [None; MAX_UI_COMPONENTS],
            capacity,
            last: Duration::ZERO,
            suspended: None,
            disposed: false,
        })
    }
    fn check(&self, now: Duration) -> Result<(), String> {
        if self.disposed || now < self.last {
            return Err("disposed motion owner or regressed time".into());
        }
        Ok(())
    }
    pub fn validate_time(&self, now: Duration) -> Result<(), String> {
        self.check(now)
    }
    /// Stage every replacement before changing any track. Screen-local progress
    /// survives renderer slot reclamation and receives fresh allocation epochs.
    pub fn rebind(&mut self, scene: &Scene) -> Result<(), String> {
        if self.disposed {
            return Err("disposed motion owner".into());
        }
        let mut next = self.tracks;
        for track in next.iter_mut().flatten() {
            track.id = scene
                .component_live(track.key)
                .ok_or("motion component not bound")?;
        }
        self.tracks = next;
        Ok(())
    }
    /// Restore remembered poses using current live binding epochs. Every pose
    /// and active track is staged before one atomic uniform update; completed
    /// poses need no active track and do not change scheduler time or lifecycle.
    pub fn restore_poses(
        &mut self,
        scene: &mut Scene,
        poses: &[(NodeId, UiTransform)],
    ) -> Result<bool, String> {
        if self.disposed
            || poses.len() > MAX_UI_COMPONENTS
            || poses
                .iter()
                .enumerate()
                .any(|(i, (node, _))| poses[..i].iter().any(|(old, _)| old == node))
        {
            return Err("invalid remembered motion pose set".into());
        }
        let mut scheduler = self.clone();
        // Tracks not listed among remembered poses still require live bindings.
        scheduler.rebind(scene)?;
        let changed = if let Some(&(node, pose)) = poses.first() {
            let first = scene
                .component_live(UiComponentKey {
                    screen: self.screen,
                    node,
                })
                .ok_or("motion pose component not bound")?;
            let mut updates = [(first, pose); MAX_UI_COMPONENTS];
            for (index, &(node, pose)) in poses.iter().enumerate() {
                UiTransform::new(pose.offset(), pose.scale(), pose.opacity())?;
                let id = scene
                    .component_live(UiComponentKey {
                        screen: self.screen,
                        node,
                    })
                    .ok_or("motion pose component not bound")?;
                updates[index] = (id, pose);
            }
            scene.set_component_transforms(&updates[..poses.len()])?
        } else {
            // Even an empty restore must reject a failed scene before publishing
            // the rebound scheduler, while retaining all active-track checks.
            scene.set_component_transforms(&[])?
        };
        *self = scheduler;
        Ok(changed)
    }
    pub fn schedule(
        &mut self,
        key: UiComponentKey,
        id: UiComponentId,
        motion: ComponentMotion,
        now: Duration,
    ) -> Result<(), String> {
        self.check(now)?;
        if key.screen != self.screen || key != id.key() || self.suspended.is_some() {
            return Err("foreign or suspended motion owner".into());
        }
        let slot = self.tracks[..self.capacity]
            .iter()
            .position(|v| v.is_some_and(|v| v.key.node == key.node))
            .or_else(|| {
                self.tracks[..self.capacity]
                    .iter()
                    .position(Option::is_none)
            })
            .ok_or("motion track capacity exhausted")?;
        self.tracks[slot] = Some(Track {
            key,
            id,
            motion,
            start: now,
        });
        self.last = now;
        Ok(())
    }
    pub fn tick(
        &mut self,
        screen: ScreenInstanceId,
        now: Duration,
        scene: &mut Scene,
    ) -> Result<bool, String> {
        self.check(now)?;
        if screen != self.screen {
            return Err("foreign motion tick owner".into());
        }
        if self.suspended.is_some() {
            self.last = now;
            return Ok(false);
        }
        let mut updates = [None; MAX_UI_COMPONENTS];
        let mut count = 0;
        for track in self.tracks.iter().flatten() {
            updates[count] = Some((track.id, track.motion.sample(now - track.start)));
            count += 1;
        }
        // All IDs are preflighted before publishing any component update.
        if updates[..count]
            .iter()
            .flatten()
            .any(|(id, _)| scene.component_transform(*id).is_none())
        {
            return Err("stale motion component binding".into());
        }
        let mut changed = false;
        for update in updates[..count].iter().flatten() {
            changed |= scene.set_component_transforms(std::slice::from_ref(update))?;
        }
        for track in &mut self.tracks {
            if track.is_some_and(|t| now - t.start >= t.motion.duration) {
                *track = None;
            }
        }
        self.last = now;
        Ok(changed)
    }
    pub fn cancel(&mut self, node: NodeId) -> bool {
        let mut cancelled = false;
        for track in &mut self.tracks {
            if track.is_some_and(|v| v.key.node == node) {
                *track = None;
                cancelled = true;
            }
        }
        cancelled
    }
    pub fn suspend(&mut self, now: Duration) -> Result<(), String> {
        self.check(now)?;
        if self.suspended.is_none() {
            self.suspended = Some(now);
        }
        self.last = now;
        Ok(())
    }
    pub fn resume(&mut self, now: Duration) -> Result<(), String> {
        self.check(now)?;
        if let Some(paused) = self.suspended {
            let shift = now - paused;
            if self
                .tracks
                .iter()
                .flatten()
                .any(|t| t.start.checked_add(shift).is_none())
            {
                return Err("motion resume time overflow".into());
            }
            for track in self.tracks.iter_mut().flatten() {
                track.start += shift;
            }
            self.suspended = None;
        }
        self.last = now;
        Ok(())
    }
    pub fn dispose(&mut self) -> usize {
        let count = self.active_count();
        self.tracks.fill(None);
        self.disposed = true;
        self.suspended = None;
        count
    }
    pub fn active_count(&self) -> usize {
        self.tracks.iter().flatten().count()
    }
    pub const fn disposed(&self) -> bool {
        self.disposed
    }
}

#[cfg(test)]
#[path = "component_motion_fixtures.rs"]
mod component_fixtures;
