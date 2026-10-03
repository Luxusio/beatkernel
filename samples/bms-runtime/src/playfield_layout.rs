//! Shared full-size playfield geometry for rendering and projected touch routing.

/// Logical browser scene dimensions before viewport scaling.
pub const LOGICAL_EXTENT: [u32; 2] = [960, 720];
/// Full playfield x/y/width/height, including head overhang and lane labels.
pub const DEFAULT_BOUNDS: [i64; 4] = [80, 106, 640, 528];

/// Partitions a validated layout using the renderer's exact integer boundaries.
/// Callers supply a valid lane index and representable nonnegative bounds.
pub fn partition_lane(index: usize, lanes: usize, x: i64, width: i64) -> (i64, i64) {
    let left = x + (index as i128 * i128::from(width) / lanes.max(1) as i128) as i64;
    let right = x + ((index + 1) as i128 * i128::from(width) / lanes.max(1) as i128) as i64;
    (left, right)
}

/// Ordered min-x/min-y/max-x/max-y touch rows for actual prepared BMS lanes.
/// Each half-open slot includes its separator pixel and lane label area.
/// Empty charts are valid; setup allocates fallibly and accepts at most 18 lanes.
pub fn default_touch_bounds(lanes: &[u8]) -> Result<Vec<f32>, String> {
    if lanes.len() > 18 {
        return Err("touch layout exceeds eighteen lanes".into());
    }
    for (index, lane) in lanes.iter().enumerate() {
        if !matches!(*lane, 0x11..=0x19 | 0x21..=0x29) || lanes[..index].contains(lane) {
            return Err("touch layout requires unique valid BMS lanes".into());
        }
    }
    let mut bounds = Vec::new();
    bounds
        .try_reserve_exact(lanes.len() * 4)
        .map_err(|_| "touch layout allocation failed")?;
    for index in 0..lanes.len() {
        let (left, right) =
            partition_lane(index, lanes.len(), DEFAULT_BOUNDS[0], DEFAULT_BOUNDS[2]);
        bounds.extend_from_slice(&[
            left as f32,
            (DEFAULT_BOUNDS[1] + 4) as f32,
            right as f32,
            (DEFAULT_BOUNDS[1] + DEFAULT_BOUNDS[3]) as f32,
        ]);
    }
    Ok(bounds)
}
