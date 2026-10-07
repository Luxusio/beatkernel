//! Shared integer contain geometry for presentation and input projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    rect: [u32; 4],
    logical: [u32; 2],
}

fn project_axis(offset: f64, physical: u32, logical: u32) -> f64 {
    if physical == logical {
        return offset;
    }
    let scaled = offset * f64::from(logical);
    if scaled.is_finite() {
        scaled / f64::from(physical)
    } else {
        // A huge captured coordinate may still have a finite downscaled result.
        offset / f64::from(physical) * f64::from(logical)
    }
}

impl Viewport {
    pub fn new(physical: [u32; 2], logical: [u32; 2]) -> Result<Self, String> {
        if physical.contains(&0) || logical.contains(&0) {
            return Err("viewport extents must be positive".into());
        }
        let [width, height] = physical;
        let [logical_width, logical_height] = logical;
        let (fitted_width, fitted_height) = if u64::from(width) * u64::from(logical_height)
            <= u64::from(height) * u64::from(logical_width)
        {
            (
                width,
                (u64::from(width) * u64::from(logical_height) / u64::from(logical_width)).max(1)
                    as u32,
            )
        } else {
            (
                (u64::from(height) * u64::from(logical_width) / u64::from(logical_height)).max(1)
                    as u32,
                height,
            )
        };
        Ok(Self {
            rect: [
                (width - fitted_width) / 2,
                (height - fitted_height) / 2,
                fitted_width,
                fitted_height,
            ],
            logical,
        })
    }

    pub fn rect(self) -> [u32; 4] {
        self.rect
    }

    /// Menu hit testing excludes the bars and the right/bottom edges.
    pub fn project(self, point: (f64, f64)) -> Option<(f64, f64)> {
        let [x, y, width, height] = self.rect;
        if !point.0.is_finite()
            || !point.1.is_finite()
            || point.0 < f64::from(x)
            || point.1 < f64::from(y)
            || point.0 >= f64::from(x) + f64::from(width)
            || point.1 >= f64::from(y) + f64::from(height)
        {
            return None;
        }
        self.project_unclipped(point).ok()
    }

    /// Contacts keep off-field positions so their original owner receives releases.
    pub fn project_unclipped(self, point: (f64, f64)) -> Result<(f64, f64), String> {
        if !point.0.is_finite() || !point.1.is_finite() {
            return Err("viewport position must be finite".into());
        }
        let [x, y, width, height] = self.rect;
        let projected = (
            project_axis(point.0 - f64::from(x), width, self.logical[0]),
            project_axis(point.1 - f64::from(y), height, self.logical[1]),
        );
        if !projected.0.is_finite() || !projected.1.is_finite() {
            return Err("viewport position overflows logical coordinates".into());
        }
        Ok(projected)
    }
}
