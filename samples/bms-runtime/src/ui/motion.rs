//! Pure presentation sampling; the caller owns elapsed time and scheduling.

use crate::scene::UiTranslation;
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
