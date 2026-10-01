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
