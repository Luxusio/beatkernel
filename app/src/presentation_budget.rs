//! Pure source and retained-upload extents, shared across presentation adapters.

pub(crate) const MAX_PLAYFIELDS: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SourceExtent {
    pub total_items: usize,
    pub max_instances: usize,
}

/// Notes, mines and both visual timelines share the original source admission.
pub(crate) fn source_extent(
    notes: usize,
    mines: usize,
    bga: usize,
    opacity: usize,
) -> Result<SourceExtent, &'static str> {
    let total_items = [notes, mines, bga, opacity]
        .into_iter()
        .try_fold(0usize, |total, count| total.checked_add(count))
        .ok_or("presentation source item count overflow")?;
    if total_items > beatkernel::chart::MAX_SOURCE_ITEMS {
        return Err("presentation source item capacity exceeded");
    }
    let max_instances = notes
        .checked_mul(3)
        .and_then(|count| count.checked_add(mines))
        .ok_or("presentation instance count overflow")?;
    Ok(SourceExtent {
        total_items,
        max_instances,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InstanceExtent {
    pub draw_count: u32,
    pub bytes: u64,
}

/// The stride is numeric so CPU accounting and device preflight use one check.
pub(crate) fn instance_extent(
    count: usize,
    instance_size: usize,
) -> Result<InstanceExtent, &'static str> {
    if instance_size == 0 {
        return Err("presentation instance size must be nonzero");
    }
    let draw_count = u32::try_from(count).map_err(|_| "presentation draw count exceeds u32")?;
    let bytes = count
        .checked_mul(instance_size)
        .ok_or("presentation instance byte count overflow")?;
    let bytes = u64::try_from(bytes).map_err(|_| "presentation buffer size exceeds u64")?;
    Ok(InstanceExtent { draw_count, bytes })
}

/// Start with one instance of storage rather than reserving the source maximum.
pub(crate) fn initial_buffer_bytes(
    instance_size: usize,
    max_buffer_size: u64,
) -> Result<u64, &'static str> {
    let bytes = instance_extent(1, instance_size)?.bytes;
    if bytes > max_buffer_size {
        return Err("presentation buffer exceeds device max_buffer_size");
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UploadPlan {
    pub draw_count: u32,
    pub bytes: u64,
    pub buffer_bytes: u64,
    pub replace_buffer: bool,
}

/// Admit the complete frame before any adapter acquires, allocates or uploads.
/// Fixed slots retain warmed capacities without allocating a temporary plan Vec.
pub(crate) fn upload_plans(
    counts: &[usize],
    retained_bytes: &[u64; MAX_PLAYFIELDS],
    instance_size: usize,
    max_buffer_size: u64,
) -> Result<[Option<UploadPlan>; MAX_PLAYFIELDS], &'static str> {
    if counts.len() > MAX_PLAYFIELDS {
        return Err("presentation exceeds displayed playfield capacity");
    }
    let minimum_bytes = initial_buffer_bytes(instance_size, max_buffer_size)?;
    let max_instances = source_extent(beatkernel::chart::MAX_SOURCE_ITEMS, 0, 0, 0)?.max_instances;
    let max_instances =
        u64::try_from(max_instances).map_err(|_| "presentation instance count exceeds u64")?;
    // An unrepresentable theoretical ceiling permits every representable byte
    // extent; each actual requirement still passes instance_extent's checks.
    let source_max_bytes = max_instances.checked_mul(minimum_bytes).unwrap_or(u64::MAX);
    let capacity_limit = source_max_bytes.min(max_buffer_size);
    let mut plans = [None; MAX_PLAYFIELDS];
    for (slot, &count) in counts.iter().enumerate() {
        let extent = instance_extent(count, instance_size)?;
        if extent.bytes > source_max_bytes || retained_bytes[slot] > source_max_bytes {
            return Err("presentation buffer exceeds source geometry capacity");
        }
        if extent.bytes > max_buffer_size || retained_bytes[slot] > max_buffer_size {
            return Err("presentation buffer exceeds device max_buffer_size");
        }
        let required_bytes = extent.bytes.max(minimum_bytes);
        let buffer_bytes = if required_bytes > retained_bytes[slot] {
            // Keep an empty field's initial storage minimal. Positive growth is
            // amortized, with slack clamped so a fitting requirement succeeds.
            if extent.bytes == 0 {
                minimum_bytes
            } else {
                required_bytes
                    .checked_next_power_of_two()
                    .unwrap_or(capacity_limit)
                    .min(capacity_limit)
            }
        } else {
            retained_bytes[slot]
        };
        plans[slot] = Some(UploadPlan {
            draw_count: extent.draw_count,
            bytes: extent.bytes,
            buffer_bytes,
            replace_buffer: buffer_bytes > retained_bytes[slot],
        });
    }
    Ok(plans)
}
