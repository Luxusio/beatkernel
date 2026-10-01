//! Portable menu hit testing and gesture ownership, with no platform input clocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlId(pub u64);

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}
impl Bounds {
    pub fn contains(self, point: (f64, f64)) -> bool {
        self.width > 0
            && self.height > 0
            && point.0.is_finite()
            && point.1.is_finite()
            && point.0 >= self.x as f64
            && point.1 >= self.y as f64
            && point.0 < self.x.saturating_add(self.width) as f64
            && point.1 < self.y.saturating_add(self.height) as f64
    }
}

/// Matches the renderer's current full-surface stretch; edges are half-open.
pub fn logical_point(
    point: (f64, f64),
    physical: (u32, u32),
    logical: (u32, u32),
) -> Option<(f64, f64)> {
    let (width, height) = physical;
    if width == 0
        || height == 0
        || logical.0 == 0
        || logical.1 == 0
        || !point.0.is_finite()
        || !point.1.is_finite()
        || point.0 < 0.0
        || point.1 < 0.0
        || point.0 >= f64::from(width)
        || point.1 >= f64::from(height)
    {
        return None;
    }
    Some((
        point.0 * f64::from(logical.0) / f64::from(width),
        point.1 * f64::from(logical.1) / f64::from(height),
    ))
}

/// Accumulates normalized wheel lines without device or clock ownership.
/// Each event emits at most 15 signed steps. Movement beyond that cap is
/// discarded rather than retained as a backlog for later unrelated events.
#[derive(Clone, Copy, Debug, Default)]
pub struct WheelSteps {
    remainder: f64,
}
impl WheelSteps {
    /// Positive lines return positive steps; fractional reversal cancels the
    /// current remainder. Nonfinite input clears the remainder and emits zero.
    pub fn push(&mut self, lines: f64) -> i32 {
        if !lines.is_finite() {
            self.reset();
            return 0;
        }
        let total = (self.remainder + lines).clamp(-15.0, 15.0);
        let steps = total.trunc();
        self.remainder = total - steps;
        steps as i32
    }
    /// Clears fractional movement when its admitting UI scope changes.
    pub fn reset(&mut self) {
        self.remainder = 0.0;
    }
}

#[derive(Default)]
pub struct Gesture {
    armed: Option<ControlId>,
}
impl Gesture {
    pub fn press(&mut self, hit: Option<ControlId>) {
        self.armed = hit;
    }
    pub fn release(&mut self, hit: Option<ControlId>) -> Option<ControlId> {
        let armed = self.armed.take();
        armed.filter(|id| Some(*id) == hit)
    }
    pub fn cancel(&mut self) {
        self.armed = None;
    }
    pub fn is_armed(&self, id: ControlId) -> bool {
        self.armed == Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wheel_fractions_emit_signed_whole_steps_and_reversal_cancels_remainder() {
        let mut wheel = WheelSteps::default();
        for _ in 0..3 {
            assert_eq!(wheel.push(0.25), 0);
        }
        assert_eq!(wheel.push(0.25), 1);
        assert_eq!(wheel.push(-0.75), 0);
        assert_eq!(wheel.push(-0.25), -1);
        assert_eq!(wheel.push(0.75), 0);
        assert_eq!(wheel.push(-0.25), 0);
        assert_eq!(wheel.push(-0.75), 0);
        assert_eq!(wheel.push(-0.75), -1);
        assert_eq!(wheel.push(0.5), 0);
        assert_eq!(wheel.push(-0.5), 0);
        assert_eq!(wheel.push(0.0), 0);
        assert_eq!(wheel.push(2.75), 2);
        assert_eq!(wheel.push(0.25), 1);
    }
    #[test]
    fn wheel_reset_and_nonfinite_events_remove_partial_movement() {
        let mut wheel = WheelSteps::default();
        wheel.push(0.75);
        wheel.reset();
        assert_eq!(wheel.push(0.25), 0);
        assert_eq!(wheel.push(0.75), 1);
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(wheel.push(-0.75), 0);
            assert_eq!(wheel.push(invalid), 0);
            assert_eq!(wheel.push(-0.25), 0);
            assert_eq!(wheel.push(-0.75), -1);
        }
    }
    #[test]
    fn wheel_extreme_events_cap_once_without_delayed_steps_or_excess_fraction() {
        let mut wheel = WheelSteps::default();
        for (lines, expected) in [
            (f64::MAX, 15),
            (-f64::MAX, -15),
            (1e100, 15),
            (-1e100, -15),
            (15.75, 15),
            (-15.75, -15),
        ] {
            wheel.push(0.75);
            assert_eq!(wheel.push(lines), expected);
            assert_eq!(wheel.push(0.0), 0);
            assert_eq!(wheel.push(0.25), 0);
            assert_eq!(wheel.push(0.75), 1);
        }
        assert_eq!(wheel.push(15.0), 15);
        assert_eq!(wheel.push(-15.0), -15);
        assert_eq!(wheel.push(-0.0), 0);
    }
    #[test]
    fn pointer_mapping_matches_stretch_and_excludes_invalid_extents_and_edges() {
        assert_eq!(
            logical_point((960.0, 360.0), (1920, 720), (960, 720)),
            Some((480.0, 360.0))
        );
        for point in [
            (f64::NAN, 1.0),
            (1.0, f64::INFINITY),
            (-1.0, 0.0),
            (1920.0, 1.0),
        ] {
            assert!(logical_point(point, (1920, 720), (960, 720)).is_none());
        }
        assert!(logical_point((1.0, 1.0), (0, 720), (960, 720)).is_none());
        let bounds = Bounds {
            x: 10,
            y: 10,
            width: 20,
            height: 20,
        };
        assert!(bounds.contains((10.0, 10.0)));
        assert!(!bounds.contains((30.0, 10.0)));
    }
    #[test]
    fn activation_requires_matching_press_and_release_and_cancellation_is_final() {
        let mut gesture = Gesture::default();
        let a = ControlId(1);
        let b = ControlId(2);
        assert_eq!(gesture.release(Some(a)), None);
        gesture.press(Some(a));
        assert_eq!(gesture.release(Some(b)), None);
        gesture.press(Some(a));
        gesture.cancel();
        assert_eq!(gesture.release(Some(a)), None);
        gesture.press(Some(a));
        assert_eq!(gesture.release(Some(a)), Some(a));
        assert_eq!(gesture.release(Some(a)), None);
    }
}
