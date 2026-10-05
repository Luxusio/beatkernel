//! Deferred immutable sharing, independent checkpoints, and canonical framing.
use super::*;
use crate::{
    chart::{SourceChart, Bpm},
    input::{ButtonEvent, DeviceId, PhysicalControlId},
    time::{ClockDomainId, ClockPoint, Duration},
};
use crate::judge::{HazardId, HazardMarker, HazardOutcome, JudgeGrade, JudgeWindow};
use std::sync::Arc;
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn marker(id: u64, at: i64, value: u64) -> HazardMarker {
    HazardMarker {
        id: HazardId(id),
        at: ts(at),
        control: GameControlId(1),
        value,
    }
}
fn plain() -> JudgeEngine {
    let chart = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap())
        .unwrap()
        .compile()
        .unwrap();
    JudgeEngine::new(
        chart,
        vec![],
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(u32::MAX),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap()
}
fn judge(markers: Vec<HazardMarker>) -> JudgeEngine {
    let count = markers.len();
    let mut engine = plain();
    engine
        .configure_hazards(HazardTimeline::new(markers, count).unwrap())
        .unwrap();
    engine
}
fn press() -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(1),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(
                DeviceId(u64::MAX),
                ClockPoint {
                    domain: ClockDomainId(7),
                    timestamp: ts(0),
                },
                u64::MAX,
            ),
            control: PhysicalControlId::keyboard(4),
            state: ButtonState::Down,
        }),
    }
}
#[test]
fn immutable_timeline_clones_share_markers_and_keep_stable_ties_signed_times_and_bounds() {
    let timeline = HazardTimeline::new(
        vec![
            marker(u64::MAX, 10, u64::MAX),
            marker(7, i64::MIN, 1),
            marker(91, 10, 2),
            marker(0, i64::MAX, 3),
        ],
        4,
    )
    .unwrap();
    let cloned = timeline.clone();
    assert_eq!(timeline.markers().as_ptr(), cloned.markers().as_ptr());
    assert_eq!(
        cloned
            .markers()
            .iter()
            .map(|marker| marker.id.0)
            .collect::<Vec<_>>(),
        [7, u64::MAX, 91, 0]
    );
    assert_eq!(timeline, cloned);
    assert_eq!(cloned.markers()[1].value, u64::MAX);
    assert!(matches!(
        HazardTimeline::new(vec![marker(7, 0, 1)], 0),
        Err(HazardError::Capacity)
    ));
    assert!(matches!(
        HazardTimeline::new(vec![marker(7, 0, 1), marker(7, 1, 2)], 2),
        Err(HazardError::DuplicateId { id: HazardId(7) })
    ));
    assert!(HazardTimeline::new(vec![], 0).unwrap().markers().is_empty());
}
#[test]
fn checkpoint_forks_share_initial_bytes_but_keep_occupancy_cursor_and_last_events_independent() {
    let original = judge(vec![marker(7, 5, 1), marker(u64::MAX, 10, 1295)]);
    let snapshot = original.snapshot().unwrap();
    let mut occupied = JudgeEngine::from_snapshot(&snapshot).unwrap();
    let mut unoccupied = JudgeEngine::from_snapshot(&snapshot).unwrap();
    assert!(Arc::ptr_eq(
        original.initial_configuration.as_ref().unwrap(),
        snapshot.engine.initial_configuration.as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        occupied.initial_configuration.as_ref().unwrap(),
        unoccupied.initial_configuration.as_ref().unwrap()
    ));
    occupied.push_input(&press(), ts(0)).unwrap();
    occupied.advance_to(ts(10)).unwrap();
    assert!(
        occupied
            .hazard_events()
            .iter()
            .all(|event| event.outcome == HazardOutcome::Triggered)
    );
    assert!(unoccupied.hazard_events().is_empty());
    assert!(snapshot.engine.hazard_events().is_empty());
    unoccupied.advance_to(ts(10)).unwrap();
    assert!(
        unoccupied
            .hazard_events()
            .iter()
            .all(|event| event.outcome == HazardOutcome::Avoided)
    );
    assert_ne!(
        occupied.hazard_events().as_ptr(),
        unoccupied.hazard_events().as_ptr()
    );
    assert_ne!(
        occupied.stable_hash().unwrap(),
        unoccupied.stable_hash().unwrap()
    );
    occupied.restore(&snapshot).unwrap();
    assert_eq!(
        occupied.stable_hash().unwrap(),
        original.stable_hash().unwrap()
    );
    occupied.advance_to(ts(10)).unwrap();
    assert_eq!(occupied.hazard_events(), unoccupied.hazard_events());
}
#[test]
fn equivalent_separately_allocated_configuration_restores_by_value_and_incompatible_refusal_is_atomic()
 {
    let first = judge(vec![marker(7, 5, 1)]);
    let snapshot = first.snapshot().unwrap();
    let mut target = judge(vec![marker(7, 5, 1)]);
    assert!(!Arc::ptr_eq(
        first.initial_configuration.as_ref().unwrap(),
        target.initial_configuration.as_ref().unwrap()
    ));
    target.push_input(&press(), ts(0)).unwrap();
    target.advance_to(ts(5)).unwrap();
    target.restore(&snapshot).unwrap();
    assert_eq!(target.stable_hash().unwrap(), first.stable_hash().unwrap());
    target.push_input(&press(), ts(0)).unwrap();
    target.advance_to(ts(5)).unwrap();
    let before = target.stable_hash().unwrap();
    let events = target.hazard_events().to_vec();
    let incompatible = judge(vec![marker(7, 5, 2)]).snapshot().unwrap();
    assert_eq!(
        target.restore(&incompatible),
        Err(SnapshotError::ConfigurationMismatch)
    );
    assert_eq!(target.stable_hash().unwrap(), before);
    assert_eq!(target.hazard_events(), events);
}
#[test]
fn reusable_checkpoint_preserves_empty_and_partly_used_hazard_buffer_and_deadline_capacity() {
    for consume_first in [false, true] {
        let mut source = judge(
            (0..8)
                .map(|index| marker(index, index as i64 + 1, index))
                .collect(),
        );
        source.deadlines.reserve(33);
        let capacity = source.deadlines.capacity();
        if consume_first {
            source.advance_to(ts(1)).unwrap();
            assert_eq!(source.hazard_events().len(), 1);
        }
        let snapshot = source.snapshot().unwrap();
        assert_eq!(snapshot.engine.deadlines.capacity(), capacity);
        let mut fork = JudgeEngine::from_snapshot(&snapshot).unwrap();
        assert_eq!(fork.deadlines.capacity(), capacity);
        let pointer = fork.hazard_events().as_ptr();
        fork.advance_to(ts(8)).unwrap();
        assert_eq!(
            fork.hazard_events().len(),
            if consume_first { 7 } else { 8 }
        );
        assert_eq!(fork.hazard_events().as_ptr(), pointer);
        assert_eq!(fork.deadlines.capacity(), capacity);
        assert_eq!(source.hazard_events().len(), usize::from(consume_first));
    }
}
fn blob(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(bytes);
}
fn independent_hash(initial: &[u8], current: &[u8]) -> u64 {
    let mut bytes = Vec::new();
    blob(&mut bytes, b"beatkernel-judge-complete/v2");
    blob(&mut bytes, initial);
    blob(&mut bytes, current);
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
#[test]
fn canonical_hazard_extension_and_complete_hash_use_original_owned_little_endian_framing() {
    let baseline = plain();
    let base = baseline.canonical_state_bytes().unwrap();
    assert_eq!(
        baseline.initial_configuration.as_ref().unwrap().as_ref(),
        base.as_slice()
    );
    assert_eq!(
        baseline.stable_hash().unwrap(),
        independent_hash(&base, &base)
    );
    for markers in [
        vec![],
        vec![
            marker(u64::MAX, 9_007_199_254_740_993, u64::MAX),
            marker(7, 9_007_199_254_740_993, 1),
        ],
    ] {
        let engine = judge(markers.clone());
        let mut extension = Vec::new();
        blob(&mut extension, b"judge-hazards/v1");
        extension.extend_from_slice(&(markers.len() as u64).to_le_bytes());
        for marker in &markers {
            extension.extend_from_slice(&marker.id.0.to_le_bytes());
            extension.extend_from_slice(&marker.at.as_nanos().to_le_bytes());
            extension.extend_from_slice(&marker.control.0.to_le_bytes());
            extension.extend_from_slice(&marker.value.to_le_bytes());
        }
        extension.extend_from_slice(&0u64.to_le_bytes()); // cursor
        extension.extend_from_slice(&(u64::from(!markers.is_empty())).to_le_bytes());
        if !markers.is_empty() {
            extension.extend_from_slice(&1u32.to_le_bytes());
            extension.extend_from_slice(&0u64.to_le_bytes());
        }
        extension.extend_from_slice(&0u64.to_le_bytes()); // event count
        let mut expected = base.clone();
        blob(&mut expected, &extension);
        assert_eq!(
            engine.initial_configuration.as_ref().unwrap().as_ref(),
            expected.as_slice()
        );
        assert_eq!(engine.canonical_state_bytes().unwrap(), expected);
        assert_eq!(
            engine.stable_hash().unwrap(),
            independent_hash(&expected, &expected)
        );
        let fork = JudgeEngine::from_snapshot(&engine.snapshot().unwrap()).unwrap();
        assert_eq!(fork.stable_hash().unwrap(), engine.stable_hash().unwrap());
        assert_ne!(
            engine.stable_hash().unwrap(),
            baseline.stable_hash().unwrap()
        );
    }
}
