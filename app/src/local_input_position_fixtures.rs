//! AC-012: acquisition projection remains attached to its exact pending event.
use super::*;
use beatkernel::input::{
    BackendId, ContactId, CustomInputEvent, EventMeta, NativeEventMeta, PhysicalControlId,
    Position2, TouchEvent, TouchPhase, VendorNamespaceId,
};

fn point(nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(10),
        timestamp: Timestamp::from_nanos(nanos),
    }
}

fn touch(id: u64, nanos: i64, sequence: u64, contact: u64) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(id), point(nanos), sequence);
    let original = ClockPoint {
        domain: ClockDomainId(99),
        timestamp: Timestamp::from_nanos(nanos - 1),
    };
    meta.native = Some(NativeEventMeta {
        backend: BackendId(2),
        code: Some(30),
        timestamp: Some(original),
    });
    meta.original_clock_point = Some(original);
    PhysicalInputEvent::Touch(TouchEvent {
        meta,
        control: PhysicalControlId::Native {
            backend: BackendId(2),
            code: 44,
        },
        contact: ContactId(contact),
        phase: TouchPhase::Down,
        position: Position2 {
            x: 1400.5,
            y: 900.25,
        },
        pressure: Some(0.75),
    })
}

fn merger(dynamic: bool, capacity: usize) -> InputMerger {
    if dynamic {
        let mut result =
            InputMerger::new_dynamic(ClockDomainId(10), point(0), 2, capacity).unwrap();
        result.register_source(DeviceId(u64::MAX)).unwrap();
        result.register_source(DeviceId(2)).unwrap();
        result
    } else {
        InputMerger::new(
            ClockDomainId(10),
            point(0),
            vec![DeviceId(u64::MAX), DeviceId(2)],
            capacity,
        )
        .unwrap()
    }
}

#[derive(Debug, PartialEq)]
struct State {
    sources: Vec<(DeviceId, Option<(Timestamp, u64)>)>,
    pending: Vec<(
        (Timestamp, DeviceId, u64, u64),
        PhysicalInputEvent,
        usize,
        Option<Position2>,
    )>,
    fixed_bytes: usize,
    payload_bytes: usize,
    ordinal: u64,
    committed: Option<Timestamp>,
    allocations: (usize, usize),
}

fn state(merge: &InputMerger) -> State {
    let mut pending: Vec<_> = merge
        .pending
        .iter()
        .map(|item| {
            (
                item.key,
                item.event.clone(),
                item.payload_bytes,
                item.position,
            )
        })
        .collect();
    pending.sort_by_key(|item| item.0);
    State {
        sources: merge
            .sources
            .iter()
            .map(|source| (source.device, source.last))
            .collect(),
        pending,
        fixed_bytes: merge.fixed_bytes,
        payload_bytes: merge.payload_bytes,
        ordinal: merge.next_ordinal,
        committed: merge.committed,
        allocations: (merge.sources.capacity(), merge.pending.capacity()),
    }
}

#[test]
fn projection_is_readonly_and_preserves_canonical_touch_and_native_metadata() {
    let mut merge = merger(true, 4);
    let original = touch(u64::MAX, 10, 7, 42);
    let projection = Position2 {
        x: -45.5,
        y: 317.25,
    };
    merge
        .admit_at(original.clone(), point(20), Some(projection))
        .unwrap();
    let before = state(&merge);
    assert_eq!(merge.peek_ready_position(point(9)).unwrap(), None);
    assert_eq!(merge.peek_ready(point(9)).unwrap(), None);
    for _ in 0..3 {
        assert_eq!(
            merge.peek_ready_position(point(10)).unwrap(),
            Some(projection)
        );
        assert_eq!(merge.peek_ready(point(10)).unwrap(), Some(&original));
        assert_eq!(state(&merge), before);
    }
    assert_eq!(merge.pop_ready(point(10)).unwrap(), Some(original));
    assert_eq!(merge.peek_ready_position(point(20)).unwrap(), None);
    assert_eq!(merge.pending(), 0);
}

#[test]
fn equal_source_time_sequence_fanout_keeps_distinct_projection_by_admission_order() {
    let mut merge = merger(false, 4);
    let projections = [
        Some(Position2 { x: -1.0, y: 7.0 }),
        None,
        Some(Position2 {
            x: 960.0,
            y: -100.0,
        }),
        Some(Position2 { x: 123.0, y: 456.0 }),
    ];
    let events: Vec<_> = (0..4).map(|contact| touch(2, 10, 5, contact)).collect();
    for (event, projection) in events.iter().zip(projections) {
        merge
            .admit_at(event.clone(), point(20), projection)
            .unwrap();
    }
    assert_eq!(merge.pending(), 4);
    for (event, projection) in events.into_iter().zip(projections) {
        assert_eq!(merge.peek_ready(point(10)).unwrap(), Some(&event));
        assert_eq!(merge.peek_ready_position(point(10)).unwrap(), projection);
        assert_eq!(merge.pop_ready(point(10)).unwrap(), Some(event));
    }
    merge.commit(point(10)).unwrap();
}

#[test]
fn unsorted_arrival_selects_position_of_matching_earliest_event() {
    let mut merge = merger(true, 4);
    let later = touch(u64::MAX, 20, 1, 1);
    let earlier = touch(2, 10, 90, 2);
    let later_projection = Position2 { x: 500.0, y: 600.0 };
    let earlier_projection = Position2 { x: -20.0, y: -30.0 };
    merge
        .admit_at(later.clone(), point(30), Some(later_projection))
        .unwrap();
    merge
        .admit_at(earlier.clone(), point(30), Some(earlier_projection))
        .unwrap();
    assert_eq!(merge.peek_ready(point(30)).unwrap(), Some(&earlier));
    assert_eq!(
        merge.peek_ready_position(point(30)).unwrap(),
        Some(earlier_projection)
    );
    assert_eq!(merge.pop_ready(point(30)).unwrap(), Some(earlier));
    assert_eq!(merge.peek_ready(point(30)).unwrap(), Some(&later));
    assert_eq!(
        merge.peek_ready_position(point(30)).unwrap(),
        Some(later_projection)
    );
    assert_eq!(merge.pop_ready(point(30)).unwrap(), Some(later));
}

#[test]
fn legacy_admit_and_explicit_none_work_in_both_fixed_and_dynamic_modes() {
    for dynamic in [false, true] {
        let mut merge = merger(dynamic, 3);
        let first = touch(2, 10, 1, 1);
        let second = touch(2, 11, 2, 2);
        merge.admit(first.clone(), point(20)).unwrap();
        merge.admit_at(second.clone(), point(20), None).unwrap();
        for event in [first, second] {
            assert_eq!(merge.peek_ready_position(point(20)).unwrap(), None);
            assert_eq!(merge.peek_ready(point(20)).unwrap(), Some(&event));
            assert_eq!(merge.pop_ready(point(20)).unwrap(), Some(event));
        }
        assert_eq!(merge.peek_ready(point(20)).unwrap(), None);
        merge.commit(point(20)).unwrap();
    }
}

#[test]
fn nonfinite_position_rejects_before_chronology_bytes_queue_or_frontier_mutation() {
    let mut merge = merger(true, 4);
    merge.commit(point(5)).unwrap();
    merge
        .admit_at(
            touch(2, 10, 7, 1),
            point(30),
            Some(Position2 { x: 1.0, y: -2.0 }),
        )
        .unwrap();
    let before = state(&merge);
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for projection in [
            Position2 { x: value, y: 10.0 },
            Position2 { x: 10.0, y: value },
        ] {
            let mut payload = Vec::with_capacity(257);
            payload.extend_from_slice(&[1, 2, 3]);
            let event = PhysicalInputEvent::Custom(CustomInputEvent {
                meta: *touch(2, 20, 90, 2).meta(),
                namespace: VendorNamespaceId(9),
                type_id: 2,
                payload,
            });
            assert_eq!(
                merge.admit_at(event, point(30), Some(projection)),
                Err(MergeError::InvalidPosition)
            );
            assert_eq!(state(&merge), before);
        }
    }
    // Rejected high sequence/time cannot poison the source's next valid event.
    merge
        .admit_at(
            touch(2, 11, 8, 3),
            point(30),
            Some(Position2 {
                x: -f32::MAX,
                y: f32::MAX,
            }),
        )
        .unwrap();
    merge.pop_ready(point(10)).unwrap();
    assert_eq!(
        merge.peek_ready_position(point(11)).unwrap(),
        Some(Position2 {
            x: -f32::MAX,
            y: f32::MAX
        })
    );
}

#[test]
fn positioned_input_preserves_frontier_domain_late_payload_and_slot_guards() {
    let mut merge = merger(false, 1);
    merge.commit(point(5)).unwrap();
    let projection = Some(Position2 { x: -1.0, y: -2.0 });
    let before = state(&merge);
    let wrong_domain = ClockPoint {
        domain: ClockDomainId(11),
        ..point(10)
    };
    assert!(matches!(
        merge.peek_ready_position(wrong_domain),
        Err(MergeError::DomainMismatch { .. })
    ));
    assert!(matches!(
        merge.peek_ready_position(point(4)),
        Err(MergeError::FrontierRegression { .. })
    ));
    assert!(matches!(
        merge.admit_at(touch(2, 5, 1, 1), point(20), projection),
        Err(MergeError::LateInput { .. })
    ));
    assert!(matches!(
        merge.admit_at(touch(2, 10, 1, 1), wrong_domain, projection),
        Err(MergeError::DomainMismatch { .. })
    ));
    assert_eq!(state(&merge), before);
    let oversized = PhysicalInputEvent::Custom(CustomInputEvent {
        meta: *touch(2, 10, 1, 1).meta(),
        namespace: VendorNamespaceId(9),
        type_id: 2,
        payload: Vec::with_capacity(MAX_PENDING_BYTES),
    });
    assert_eq!(
        merge.admit_at(oversized, point(20), projection),
        Err(MergeError::StorageCapacity)
    );
    assert_eq!(state(&merge), before);
    merge
        .admit_at(touch(2, 10, 1, 1), point(20), projection)
        .unwrap();
    let full = state(&merge);
    assert_eq!(
        merge.admit_at(
            touch(2, 11, 2, 2),
            point(20),
            Some(Position2 { x: 99.0, y: 98.0 })
        ),
        Err(MergeError::Capacity)
    );
    assert!(matches!(
        merge.commit(point(10)),
        Err(MergeError::PendingBeforeFrontier { .. })
    ));
    assert_eq!(state(&merge), full);
    assert_eq!(merge.peek_ready_position(point(10)).unwrap(), projection);
    merge.pop_ready(point(10)).unwrap();
    merge.admit_at(touch(2, 11, 2, 2), point(20), None).unwrap();
    assert_eq!(merge.peek_ready_position(point(11)).unwrap(), None);
}

#[test]
fn positioned_custom_payload_retains_original_allocation_and_acquisition_provenance() {
    let mut merge = merger(true, 2);
    let mut payload = Vec::with_capacity(257);
    payload.extend_from_slice(&[0, 255, 7, 42]);
    let pointer = payload.as_ptr();
    let capacity = payload.capacity();
    let event = PhysicalInputEvent::Custom(CustomInputEvent {
        meta: *touch(u64::MAX, 10, 77, 3).meta(),
        namespace: VendorNamespaceId(9),
        type_id: 2,
        payload,
    });
    let expected = event.clone();
    let projection = Some(Position2 { x: -8.5, y: 720.25 });
    merge.admit_at(event, point(20), projection).unwrap();
    let before = state(&merge);
    assert_eq!(merge.peek_ready_position(point(10)).unwrap(), projection);
    assert_eq!(merge.peek_ready(point(10)).unwrap(), Some(&expected));
    assert_eq!(state(&merge), before);
    assert_eq!(merge.payload_bytes, capacity);
    let returned = merge.pop_ready(point(10)).unwrap().unwrap();
    assert_eq!(returned, expected);
    let PhysicalInputEvent::Custom(returned) = returned else {
        panic!("payload variant changed")
    };
    assert_eq!(returned.payload.as_ptr(), pointer);
    assert_eq!(returned.payload.capacity(), capacity);
    assert_eq!(merge.payload_bytes, 0);
}
