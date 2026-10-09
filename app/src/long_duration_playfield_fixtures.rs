//! Actual retained renderer-input geometry, not a GPU execution test.
use std::sync::Arc;

use beatkernel::{chart::ObjectId, time::Timestamp};

use crate::{
    player_chart::PlayerNote,
    playfield_gpu::{PlayfieldCache, PlayfieldFrame},
    ui::interaction::Bounds,
};

const TWENTY_HOURS: i64 = 72_000_000_000_000;
const WEEK: i64 = 604_800_000_000_000;
const LOOKAHEAD: i64 = 1_000_000_000;
const EPOCH_LIMIT: i64 = 250_000_000;
// The selected layout spans exactly 500 pixels per second. These are independent
// integer-ratio reference coordinates, with subtraction before conversion.
const PIXEL_TOLERANCE: f64 = 0.001;

fn bounds() -> Bounds {
    Bounds {
        x: 80,
        y: 106,
        width: 640,
        height: 528,
    }
}

fn note(id: u64, lane: usize, start: i64, end: Option<i64>) -> PlayerNote {
    PlayerNote {
        object: ObjectId(id),
        lane_index: lane,
        start: Timestamp::from_nanos(start),
        end: end.map(Timestamp::from_nanos),
    }
}

fn chart(extent: i64) -> (Vec<PlayerNote>, [i64; 3]) {
    let probes = [0, extent / 2, extent - 500_000_000];
    // One genuine hold traverses the entire chart, alongside nearby real heads
    // and tails at each sampled position. This is not a shifted short chart.
    let mut notes = vec![note(1, 0, 0, Some(extent))];
    for (index, &at) in probes.iter().enumerate() {
        notes.push(note(
            2 + index as u64 * 2,
            1,
            at + 1_000_000,
            Some(at + 200_000_000),
        ));
        notes.push(note(3 + index as u64 * 2, 1, at + 123_456_789, None));
    }
    (notes, probes)
}

fn pixel(time: Timestamp, now: i64) -> f64 {
    let delta = i128::from(time.as_nanos()) - i128::from(now);
    // Rational (610 * 2,000,000 - delta) / 2,000,000; no absolute float times.
    (610_i128 * 2_000_000 - delta) as f64 / 2_000_000.0
}

fn close(actual: f32, expected: f64) {
    assert!(actual.is_finite());
    assert!(
        (f64::from(actual) - expected).abs() <= PIXEL_TOLERANCE,
        "actual {actual}, expected {expected}"
    );
}

fn frame(
    cache: &mut PlayfieldCache,
    notes: &[PlayerNote],
    probe_index: usize,
    now: i64,
) -> PlayfieldFrame {
    cache.frame_indexed_with_progress(
        notes,
        &[0, 1 + probe_index * 2, 2 + probe_index * 2],
        2,
        bounds(),
        Timestamp::from_nanos(now),
        LOOKAHEAD,
        None,
    )
}

fn assert_local_geometry(frame: &PlayfieldFrame, notes: &[PlayerNote], index: usize, now: i64) {
    assert_eq!(frame.instances.len(), 7);
    assert_eq!((frame.top, frame.bottom), (110.0, 625.0));
    let hold = &notes[1 + index * 2];
    let tap = &notes[2 + index * 2];
    for (instance, kind) in frame.instances[3..6].iter().zip([0.0, 1.0, 2.0]) {
        assert_eq!(instance.appearance[0], kind);
        close(instance.geometry[2] + frame.drift, pixel(hold.start, now));
        close(
            instance.geometry[3] + frame.drift,
            pixel(hold.end.expect("local hold tail"), now),
        );
    }
    assert_eq!(frame.instances[6].appearance[0], 2.0);
    close(
        frame.instances[6].geometry[2] + frame.drift,
        pixel(tap.start, now),
    );
    close(
        frame.instances[6].geometry[3] + frame.drift,
        pixel(tap.start, now),
    );
}

fn assert_spanning_body(frame: &PlayfieldFrame, spanning: &PlayerNote, now: i64) {
    assert!(frame.drift.is_finite());
    assert!(frame
        .instances
        .iter()
        .flat_map(|instance| instance.geometry)
        .all(f32::is_finite));
    let body = &frame.instances[0];
    assert_eq!(body.appearance[0], 0.0);
    // The production shader receives these endpoints and drift separately.
    // Far endpoints may saturate, but the visible body's clipped intersection
    // must still match its true integer-time extent.
    let actual_top = (body.geometry[3] + frame.drift).max(frame.top);
    let actual_bottom = actual_top.max((body.geometry[2] + frame.drift).min(frame.bottom));
    let expected_top = pixel(spanning.end.expect("spanning tail"), now).max(110.0);
    let expected_bottom = expected_top.max(pixel(spanning.start, now).min(625.0));
    close(actual_top, expected_top);
    close(actual_bottom, expected_bottom);
}

#[test]
fn true_twenty_hour_and_week_extents_preserve_local_geometry_across_skipped_frames() {
    for extent in [TWENTY_HOURS, WEEK] {
        let (notes, probes) = chart(extent);
        let original = notes.clone();
        let mut cache = PlayfieldCache::default();
        let mut previous: Option<PlayfieldFrame> = None;
        for (index, now) in probes.into_iter().enumerate() {
            let current = frame(&mut cache, &notes, index, now);
            assert_eq!(current.drift, 0.0);
            assert_local_geometry(&current, &notes, index, now);
            assert_spanning_body(&current, &notes[0], now);
            if let Some(old) = &previous {
                assert!(!Arc::ptr_eq(&old.instances, &current.instances));
            }
            previous = Some(current);
        }
        assert_eq!(
            notes, original,
            "rendering must not rewrite chart timestamps"
        );
    }
}

#[test]
fn long_extent_equal_time_and_epoch_rebase_keep_head_tail_plus_drift_continuous() {
    for extent in [TWENTY_HOURS, WEEK] {
        let (notes, probes) = chart(extent);
        let original = notes.clone();
        for (index, now) in probes.into_iter().enumerate() {
            let mut cache = PlayfieldCache::default();
            let initial = frame(&mut cache, &notes, index, now);
            let paused = frame(&mut cache, &notes, index, now);
            assert!(Arc::ptr_eq(&initial.instances, &paused.instances));
            assert_eq!(paused.drift, initial.drift);
            let boundary = frame(&mut cache, &notes, index, now + EPOCH_LIMIT);
            assert!(Arc::ptr_eq(&initial.instances, &boundary.instances));
            assert_eq!(boundary.drift, 125.0);
            assert_local_geometry(&boundary, &notes, index, now + EPOCH_LIMIT);
            assert_spanning_body(&boundary, &notes[0], now + EPOCH_LIMIT);
            let paused_boundary = frame(&mut cache, &notes, index, now + EPOCH_LIMIT);
            assert!(Arc::ptr_eq(&boundary.instances, &paused_boundary.instances));
            assert_eq!(paused_boundary.drift, 125.0);
            let rebased = frame(&mut cache, &notes, index, now + EPOCH_LIMIT + 1);
            assert!(!Arc::ptr_eq(&boundary.instances, &rebased.instances));
            assert_eq!(rebased.drift, 0.0);
            assert_local_geometry(&rebased, &notes, index, now + EPOCH_LIMIT + 1);
            assert_spanning_body(&rebased, &notes[0], now + EPOCH_LIMIT + 1);
            for (before, after) in boundary.instances[3..].iter().zip(&rebased.instances[3..]) {
                for coordinate in [2, 3] {
                    close(
                        after.geometry[coordinate] + rebased.drift,
                        f64::from(before.geometry[coordinate] + boundary.drift) + 0.000_000_5,
                    );
                }
            }
        }
        assert_eq!(notes, original);
    }
}

#[test]
fn long_extent_reverse_seek_rebuilds_geometry_without_mutating_source_time() {
    for extent in [TWENTY_HOURS, WEEK] {
        let (notes, probes) = chart(extent);
        let original = notes.clone();
        let mut cache = PlayfieldCache::default();
        let end = frame(&mut cache, &notes, 2, probes[2]);
        let end_forward = frame(&mut cache, &notes, 2, probes[2] + 100_000_000);
        assert!(Arc::ptr_eq(&end.instances, &end_forward.instances));
        // Same selected notes, decreasing time: proves seek invalidation rather
        // than relying on the visible-note set changing at the seek destination.
        let rewound = frame(&mut cache, &notes, 2, probes[2]);
        assert!(!Arc::ptr_eq(&end_forward.instances, &rewound.instances));
        assert_eq!(rewound.drift, 0.0);
        assert_local_geometry(&rewound, &notes, 2, probes[2]);
        for index in [1, 0] {
            let sought = frame(&mut cache, &notes, index, probes[index]);
            assert!(!Arc::ptr_eq(&rewound.instances, &sought.instances));
            assert_local_geometry(&sought, &notes, index, probes[index]);
            assert_spanning_body(&sought, &notes[0], probes[index]);
        }
        assert_eq!(notes, original);
    }
}
