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
    touch_bounds(lanes, DEFAULT_BOUNDS)
}

/// One visible panel in the common one/two/four-field page layout.
/// Count is the visible page size, independently of the full gameplay roster.
pub fn local_panel_bounds(count: usize, slot: usize) -> Result<[i64; 4], String> {
    if !(1..=4).contains(&count) || slot >= count {
        return Err("local panel requires one to four visible players and a valid slot".into());
    }
    let columns = if count == 1 { 1 } else { 2 };
    let rows = if count <= 2 { 1 } else { 2 };
    let width = (912 - (columns - 1) * 12) / columns;
    let height = (540 - (rows - 1) * 12) / rows;
    Ok([
        24 + (slot % columns) as i64 * (width + 12) as i64,
        100 + (slot / columns) as i64 * (height + 12) as i64,
        width as i64,
        height as i64,
    ])
}

/// The actual field below each local panel's score header, without comparisons.
pub fn local_field_bounds(count: usize, slot: usize) -> Result<[i64; 4], String> {
    let [x, y, width, height] = local_panel_bounds(count, slot)?;
    Ok([x + 10, y + 72, width - 20, height - 80])
}

/// Contact regions in the exact visible local field's global scene coordinates.
pub fn local_touch_bounds(lanes: &[u8], count: usize, slot: usize) -> Result<Vec<f32>, String> {
    touch_bounds(lanes, local_field_bounds(count, slot)?)
}

fn touch_bounds(lanes: &[u8], field: [i64; 4]) -> Result<Vec<f32>, String> {
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
        let (left, right) = partition_lane(index, lanes.len(), field[0], field[2]);
        bounds.extend_from_slice(&[
            left as f32,
            (field[1] + 4) as f32,
            right as f32,
            (field[1] + field[3]) as f32,
        ]);
    }
    Ok(bounds)
}
