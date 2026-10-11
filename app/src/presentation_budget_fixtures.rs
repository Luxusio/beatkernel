//! Numeric admission fixtures: no chart-sized allocation or GPU is needed to
//! prove the source, draw, and requested-device boundaries.
use crate::presentation_budget::{
    initial_buffer_bytes, instance_extent, source_extent, upload_plans, MAX_PLAYFIELDS,
};
use beatkernel::chart::MAX_SOURCE_ITEMS;

// NoteInstance contains two four-component f32 vectors. Keep the oracle literal
// so a changed production size does not silently change the expected budgets.
const INSTANCE_BYTES: usize = 32;

#[cfg(feature = "graphics")]
#[test]
fn presentation_budget_literal_byte_oracle_matches_gpu_instance_layout() {
    assert_eq!(
        std::mem::size_of::<crate::playfield_gpu::NoteInstance>(),
        32
    );
}

#[test]
fn presentation_budget_empty_and_mixed_source_extents_count_all_owners() {
    let empty = source_extent(0, 0, 0, 0).unwrap();
    assert_eq!(empty.total_items, 0);
    assert_eq!(empty.max_instances, 0);
    // Seven potential holds have body, tail, and head; five mines have one
    // instance each. Timeline records occupy source admission, not geometry.
    let mixed = source_extent(7, 5, 11, 13).unwrap();
    assert_eq!(mixed.total_items, 36);
    assert_eq!(mixed.max_instances, 26);
    let timelines = source_extent(0, 0, 11, 13).unwrap();
    assert_eq!(timelines.total_items, 24);
    assert_eq!(timelines.max_instances, 0);
}

#[test]
fn presentation_budget_source_limit_is_shared_and_inclusive() {
    assert_eq!(MAX_SOURCE_ITEMS, 1_000_000);
    let mixed = source_extent(250_000, 250_000, 250_000, 250_000).unwrap();
    assert_eq!(mixed.total_items, 1_000_000);
    assert_eq!(mixed.max_instances, 1_000_000);
    for counts in [
        [250_001, 250_000, 250_000, 250_000],
        [250_000, 250_001, 250_000, 250_000],
        [250_000, 250_000, 250_001, 250_000],
        [250_000, 250_000, 250_000, 250_001],
    ] {
        assert!(source_extent(counts[0], counts[1], counts[2], counts[3]).is_err());
    }
    for slot in 0..4 {
        let mut counts = [0; 4];
        counts[slot] = MAX_SOURCE_ITEMS;
        let admitted = source_extent(counts[0], counts[1], counts[2], counts[3]).unwrap();
        assert_eq!(admitted.total_items, MAX_SOURCE_ITEMS);
        assert_eq!(admitted.max_instances, [3_000_000, 1_000_000, 0, 0][slot]);
        counts[slot] += 1;
        assert!(source_extent(counts[0], counts[1], counts[2], counts[3]).is_err());
    }
}

#[test]
fn presentation_budget_extreme_source_arithmetic_never_wraps_into_admission() {
    for counts in [
        [usize::MAX, 0, 0, 0],
        [0, usize::MAX, 0, 0],
        [0, 0, usize::MAX, 0],
        [0, 0, 0, usize::MAX],
        [usize::MAX, 1, 0, 0],
        [1, usize::MAX, 0, 0],
        [0, 0, usize::MAX, 1],
        [usize::MAX, usize::MAX, usize::MAX, usize::MAX],
    ] {
        assert!(source_extent(counts[0], counts[1], counts[2], counts[3]).is_err());
    }
}

#[test]
fn presentation_budget_upload_extent_has_no_old_visible_or_instance_ceiling() {
    for (count, expected_bytes) in [(0, 0), (1, 32), (2049, 65_568), (8193, 262_176)] {
        let extent = instance_extent(count, INSTANCE_BYTES).unwrap();
        assert_eq!(extent.draw_count, count as u32);
        assert_eq!(extent.bytes, expected_bytes);
    }
    let all_holds = source_extent(MAX_SOURCE_ITEMS, 0, 0, 0).unwrap();
    let extent = instance_extent(all_holds.max_instances, INSTANCE_BYTES).unwrap();
    assert_eq!(extent.draw_count, 3_000_000);
    assert_eq!(extent.bytes, 96_000_000);
}

#[test]
fn presentation_budget_draw_conversion_and_byte_multiplication_are_checked() {
    let last_draw = instance_extent(u32::MAX as usize, 1).unwrap();
    assert_eq!(last_draw.draw_count, u32::MAX);
    assert_eq!(last_draw.bytes, 4_294_967_295);
    if let Some(too_many) = (u32::MAX as usize).checked_add(1) {
        assert!(instance_extent(too_many, 1).is_err());
    }
    assert!(instance_extent(usize::MAX, INSTANCE_BYTES).is_err());
    assert!(instance_extent(2, usize::MAX).is_err());
    assert!(instance_extent(0, 0).is_err());
    assert!(instance_extent(1, 0).is_err());
    // The last representable usize byte extent is valid for one instance;
    // doubling it must refuse, even though the draw count itself is small.
    let last_bytes = instance_extent(1, usize::MAX).unwrap();
    assert_eq!(last_bytes.draw_count, 1);
    assert_eq!(last_bytes.bytes, usize::MAX as u64);
}

#[test]
fn presentation_budget_initial_storage_is_one_instance_and_device_bounded() {
    assert_eq!(initial_buffer_bytes(INSTANCE_BYTES, 32).unwrap(), 32);
    assert_eq!(initial_buffer_bytes(INSTANCE_BYTES, u64::MAX).unwrap(), 32);
    assert!(initial_buffer_bytes(INSTANCE_BYTES, 31).is_err());
    assert!(initial_buffer_bytes(INSTANCE_BYTES, 0).is_err());
    assert!(initial_buffer_bytes(0, u64::MAX).is_err());
}

#[test]
fn presentation_budget_actual_device_limit_accepts_exact_and_rejects_next_instance() {
    let exact = upload_plans(&[8193], &[32; 4], INSTANCE_BYTES, 262_176).unwrap();
    let first = exact[0].unwrap();
    assert_eq!(first.draw_count, 8193);
    assert_eq!(first.bytes, 262_176);
    assert_eq!(first.buffer_bytes, 262_176);
    assert!(first.replace_buffer);
    assert!(exact[1..].iter().all(Option::is_none));
    assert!(upload_plans(&[8194], &[32; 4], INSTANCE_BYTES, 262_176).is_err());
    assert!(upload_plans(&[8193], &[32; 4], INSTANCE_BYTES, 262_175).is_err());
}

#[test]
fn presentation_budget_all_four_fields_preflight_together() {
    assert_eq!(MAX_PLAYFIELDS, 4);
    let counts = [0, 1, 8193, 7];
    let retained = [32, 64, 32, 224];
    let plan = upload_plans(&counts, &retained, INSTANCE_BYTES, 262_176).unwrap();
    for (slot, (draw, bytes, capacity, replacement)) in [
        (0, 0, 32, false),
        (1, 32, 64, false),
        (8193, 262_176, 262_176, true),
        (7, 224, 224, false),
    ]
    .into_iter()
    .enumerate()
    {
        let field = plan[slot].unwrap();
        assert_eq!(field.draw_count, draw);
        assert_eq!(field.bytes, bytes);
        assert_eq!(field.buffer_bytes, capacity);
        assert_eq!(field.replace_buffer, replacement);
    }
    // The first three are admissible; an invalid fourth refuses the whole
    // candidate rather than returning a prefix the renderer could submit.
    assert!(upload_plans(&[0, 1, 8193, 8194], &retained, INSTANCE_BYTES, 262_176).is_err());
    assert!(upload_plans(
        &[0, 1, 8193, usize::MAX],
        &retained,
        INSTANCE_BYTES,
        u64::MAX
    )
    .is_err());
    assert!(upload_plans(
        &[0, 1, 8193, 0],
        &[32, 64, 32, 262_177],
        INSTANCE_BYTES,
        262_176
    )
    .is_err());
    assert_eq!(counts, [0, 1, 8193, 7]);
    assert_eq!(retained, [32, 64, 32, 224]);
    assert!(upload_plans(&[0; 5], &retained, INSTANCE_BYTES, u64::MAX).is_err());
}

#[test]
fn presentation_budget_retained_capacity_fits_and_power_of_two_growth_stays_bounded() {
    let retained = [320, 32, 96, 32];
    let plan = upload_plans(&[9, 2, 3], &retained, INSTANCE_BYTES, 320).unwrap();
    let fits = plan[0].unwrap();
    assert_eq!(
        (fits.bytes, fits.buffer_bytes, fits.replace_buffer),
        (288, 320, false)
    );
    let grows = plan[1].unwrap();
    assert_eq!(
        (grows.bytes, grows.buffer_bytes, grows.replace_buffer),
        (64, 64, true)
    );
    let exact_fit = plan[2].unwrap();
    assert_eq!(
        (
            exact_fit.bytes,
            exact_fit.buffer_bytes,
            exact_fit.replace_buffer
        ),
        (96, 96, false)
    );
    assert!(plan[3].is_none());
}

#[test]
fn presentation_budget_empty_geometry_retains_storage_and_sparse_fields_pay_actual_extent() {
    let empty = upload_plans(&[0], &[320, 32, 32, 32], INSTANCE_BYTES, 320).unwrap()[0].unwrap();
    assert_eq!(empty.draw_count, 0);
    assert_eq!(empty.bytes, 0);
    assert_eq!(empty.buffer_bytes, 320);
    assert!(!empty.replace_buffer);
    let fresh = upload_plans(&[0, 1, 3], &[0; 4], INSTANCE_BYTES, 96).unwrap();
    for (slot, bytes, capacity) in [(0, 0, 32), (1, 32, 32), (2, 96, 96)] {
        let field = fresh[slot].unwrap();
        assert_eq!(field.bytes, bytes);
        assert_eq!(field.buffer_bytes, capacity);
        assert!(field.replace_buffer);
    }
    assert!(fresh[3].is_none());
}

#[test]
fn presentation_budget_retained_buffers_must_fit_requested_device_too() {
    // A fitting draw does not authorize retaining an oversized buffer.
    assert!(upload_plans(&[1], &[64, 32, 32, 32], INSTANCE_BYTES, 32).is_err());
    assert!(upload_plans(&[0], &[64, 32, 32, 32], INSTANCE_BYTES, 32).is_err());
    assert!(upload_plans(&[0], &[0; 4], INSTANCE_BYTES, 31).is_err());
    assert!(upload_plans(&[1], &[32; 4], 0, u64::MAX).is_err());
    assert!(upload_plans(&[2], &[32; 4], usize::MAX, u64::MAX).is_err());
}

#[test]
fn presentation_budget_incremental_window_growth_amortizes_replacements() {
    let mut retained = [32; 4];
    let mut replacement_counts = Vec::new();
    for count in 1..=2048 {
        let field = upload_plans(&[count], &retained, INSTANCE_BYTES, 100_000).unwrap()[0].unwrap();
        assert_eq!(field.draw_count, count as u32);
        assert_eq!(field.bytes, count as u64 * 32);
        assert!(field.bytes <= field.buffer_bytes);
        assert!(field.buffer_bytes <= 96_000_000);
        assert!(field.buffer_bytes <= 100_000);
        assert!(field.buffer_bytes >= retained[0]);
        if field.replace_buffer {
            assert!(field.buffer_bytes > retained[0]);
            replacement_counts.push(count);
        } else {
            assert_eq!(field.buffer_bytes, retained[0]);
        }
        retained[0] = field.buffer_bytes;
    }
    // One initial instance grows at each doubling boundary. The literal
    // schedule independently distinguishes logarithmic retention from one
    // allocation for every newly visible note; no timing assertion is implied.
    assert_eq!(
        replacement_counts,
        [2, 3, 5, 9, 17, 33, 65, 129, 257, 513, 1025]
    );
    assert_eq!(replacement_counts.len(), 11);
    assert_eq!(retained[0], 65_536);
    for count in [2048, 2047, 1, 0] {
        let field = upload_plans(&[count], &retained, INSTANCE_BYTES, 100_000).unwrap()[0].unwrap();
        assert_eq!(field.buffer_bytes, 65_536);
        assert!(!field.replace_buffer);
    }
}

#[test]
fn presentation_budget_source_ceiling_clamps_growth_without_rejecting_valid_geometry() {
    // All one million source items could be holds, each with three primitives.
    // Their 96 MB bound is below the next binary growth capacity (128 MiB).
    let maximum =
        upload_plans(&[3_000_000], &[32; 4], INSTANCE_BYTES, u64::MAX).unwrap()[0].unwrap();
    assert_eq!(maximum.draw_count, 3_000_000);
    assert_eq!(maximum.bytes, 96_000_000);
    assert_eq!(maximum.buffer_bytes, 96_000_000);
    assert!(maximum.replace_buffer);
    assert!(upload_plans(&[3_000_001], &[32; 4], INSTANCE_BYTES, u64::MAX).is_err());
    // This count has just crossed 64 MiB; geometric slack must be capped by
    // source admission even when the actual device permits much more.
    let crossing =
        upload_plans(&[2_097_153], &[32; 4], INSTANCE_BYTES, u64::MAX).unwrap()[0].unwrap();
    assert_eq!(crossing.bytes, 67_108_896);
    assert_eq!(crossing.buffer_bytes, 96_000_000);
    assert!(crossing.replace_buffer);
    let retained = [96_000_000, 32, 32, 32];
    let fits = upload_plans(&[3_000_000], &retained, INSTANCE_BYTES, u64::MAX).unwrap()[0].unwrap();
    assert!(!fits.replace_buffer);
    assert_eq!(fits.buffer_bytes, 96_000_000);
    assert!(upload_plans(&[0], &[96_000_001, 32, 32, 32], INSTANCE_BYTES, u64::MAX).is_err());
    // Source admission belongs to upload planning; generic byte/draw conversion
    // remains usable independently and has no one-million-source policy.
    assert_eq!(
        instance_extent(3_000_001, INSTANCE_BYTES).unwrap().bytes,
        96_000_032
    );
}

#[test]
fn presentation_budget_awkward_device_ceiling_clamps_geometric_slack() {
    // Five 32-byte instances need 160 bytes. The device permits six instances,
    // so rounding to 256 bytes must clamp to 192 rather than reject the draw.
    let grows = upload_plans(&[5], &[32; 4], INSTANCE_BYTES, 192).unwrap()[0].unwrap();
    assert_eq!(grows.draw_count, 5);
    assert_eq!(grows.bytes, 160);
    assert_eq!(grows.buffer_bytes, 192);
    assert!(grows.replace_buffer);
    let fits = upload_plans(&[6], &[192, 32, 32, 32], INSTANCE_BYTES, 192).unwrap()[0].unwrap();
    assert_eq!(fits.bytes, 192);
    assert_eq!(fits.buffer_bytes, 192);
    assert!(!fits.replace_buffer);
    assert!(upload_plans(&[7], &[192, 32, 32, 32], INSTANCE_BYTES, 192).is_err());
    assert!(upload_plans(&[5], &[32; 4], INSTANCE_BYTES, 159).is_err());
}
