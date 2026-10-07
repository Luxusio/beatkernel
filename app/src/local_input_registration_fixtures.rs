//! AC-011 contracts: explicit registration of genuine acquired source IDs.
use super::*;
use beatkernel::input::{
    BackendId, ButtonEvent, ButtonState, CustomInputEvent, EventMeta, NativeEventMeta,
    PhysicalControlId, RawHidReportEvent, VendorNamespaceId,
};

fn point(nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(10),
        timestamp: Timestamp::from_nanos(nanos),
    }
}

fn button(device: DeviceId, nanos: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(device, point(nanos), sequence);
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
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(4),
        state,
    })
}

fn dynamic(sources: usize, slots: usize) -> InputMerger {
    InputMerger::new_dynamic(ClockDomainId(10), point(0), sources, slots).unwrap()
}

#[derive(Debug, PartialEq)]
struct State {
    sources: Vec<(DeviceId, Option<(Timestamp, u64)>)>,
    pending: Vec<((Timestamp, DeviceId, u64, u64), PhysicalInputEvent, usize)>,
    fixed_bytes: usize,
    payload_bytes: usize,
    ordinal: u64,
    committed: Option<Timestamp>,
    source_allocation: usize,
    pending_allocation: usize,
}

fn state(merge: &InputMerger) -> State {
    let mut pending: Vec<_> = merge
        .pending
        .iter()
        .map(|item| (item.key, item.event.clone(), item.payload_bytes))
        .collect();
    pending.sort_by_key(|item| item.0);
    State {
        sources: merge
            .sources
            .iter()
            .map(|item| (item.device, item.last))
            .collect(),
        pending,
        fixed_bytes: merge.fixed_bytes,
        payload_bytes: merge.payload_bytes,
        ordinal: merge.next_ordinal,
        committed: merge.committed,
        source_allocation: merge.sources.capacity(),
        pending_allocation: merge.pending.capacity(),
    }
}

#[test]
fn dynamic_setup_starts_empty_and_validates_explicit_bounds_and_domain() {
    for (sources, slots) in [(1, 1), (4096, 65536)] {
        let merge = dynamic(sources, slots);
        assert_eq!(merge.source_count(), 0);
        assert_eq!(merge.source_capacity(), sources);
        assert_eq!(merge.pending(), 0);
        assert!(merge.sources.capacity() >= sources);
        assert!(merge.pending.capacity() >= slots);
    }
    for (sources, slots) in [(0, 1), (4097, 1), (1, 0), (1, 65537)] {
        assert!(matches!(
            InputMerger::new_dynamic(ClockDomainId(10), point(0), sources, slots),
            Err(MergeError::InvalidConfiguration)
        ));
    }
    assert!(matches!(
        InputMerger::new_dynamic(ClockDomainId(11), point(0), 1, 1),
        Err(MergeError::DomainMismatch {
            expected: ClockDomainId(11),
            actual: ClockDomainId(10)
        })
    ));
}

#[test]
fn fixed_constructor_keeps_64_source_limit_and_unknown_registration_refusal() {
    let devices: Vec<_> = (0..64).rev().map(DeviceId).collect();
    let mut merge = InputMerger::new(ClockDomainId(10), point(0), devices, 2).unwrap();
    assert_eq!(merge.source_count(), 64);
    assert_eq!(merge.source_capacity(), 64);
    let before = state(&merge);
    merge.register_source(DeviceId(7)).unwrap();
    assert_eq!(state(&merge), before);
    assert_eq!(
        merge.register_source(DeviceId(u64::MAX)),
        Err(MergeError::UnknownDevice(DeviceId(u64::MAX)))
    );
    assert_eq!(state(&merge), before);
    assert_eq!(
        merge.admit(
            button(DeviceId(u64::MAX), 1, 1, ButtonState::Down),
            point(2)
        ),
        Err(MergeError::UnknownDevice(DeviceId(u64::MAX)))
    );
    assert_eq!(state(&merge), before);
    for devices in [
        vec![],
        vec![DeviceId(1), DeviceId(1)],
        (0..65).map(DeviceId).collect(),
    ] {
        assert!(matches!(
            InputMerger::new(ClockDomainId(10), point(0), devices, 2),
            Err(MergeError::InvalidConfiguration)
        ));
    }
}

#[test]
fn all_4096_source_slots_register_without_growth_or_eviction_and_saturation_is_atomic() {
    let mut merge = dynamic(4096, 2);
    let allocation = merge.sources.capacity();
    let pointer = merge.sources.as_ptr();
    let ids: Vec<_> = (0..4096)
        .rev()
        .map(|offset| DeviceId(u64::MAX - offset))
        .collect();
    for id in &ids {
        merge.register_source(*id).unwrap();
        assert_eq!(merge.sources.capacity(), allocation);
        assert_eq!(merge.sources.as_ptr(), pointer);
    }
    assert_eq!(merge.source_count(), 4096);
    assert!(
        merge
            .sources
            .windows(2)
            .all(|pair| pair[0].device < pair[1].device)
    );
    merge
        .admit(button(ids[0], 10, 2, ButtonState::Down), point(20))
        .unwrap();
    let before = state(&merge);
    assert_eq!(
        merge.register_source(DeviceId(0)),
        Err(MergeError::SourceCapacity)
    );
    assert_eq!(state(&merge), before);
    merge.register_source(ids[0]).unwrap();
    assert_eq!(state(&merge), before);
    // Every original ID remains admissible even after saturation: no eviction.
    merge.pop_ready(point(10)).unwrap();
    for id in ids {
        let event = button(id, 11, 3, ButtonState::Down);
        merge.admit(event.clone(), point(20)).unwrap();
        assert_eq!(merge.pop_ready(point(11)).unwrap(), Some(event));
        assert_eq!(merge.sources.capacity(), allocation);
        assert_eq!(merge.sources.as_ptr(), pointer);
    }
}

#[test]
fn explicit_registration_preserves_full_width_identity_and_pending_key_order() {
    let mut merge = dynamic(4, 8);
    let ids = [
        DeviceId(u64::MAX),
        DeviceId(1u64 << 63),
        DeviceId(0),
        DeviceId(17),
    ];
    let before = state(&merge);
    assert_eq!(
        merge.admit(button(ids[0], 1, 1, ButtonState::Down), point(20)),
        Err(MergeError::UnknownDevice(ids[0]))
    );
    assert_eq!(state(&merge), before);
    for id in ids {
        merge.register_source(id).unwrap();
    }
    let events = [
        button(ids[0], 20, 1, ButtonState::Down),
        button(ids[1], 10, 2, ButtonState::Down),
        button(ids[2], 10, 5, ButtonState::Down),
        button(ids[2], 10, 5, ButtonState::Up),
        button(ids[2], 10, 6, ButtonState::Repeat),
        button(ids[3], 10, 1, ButtonState::Down),
    ];
    let allocation = merge.pending.capacity();
    for event in &events {
        merge.admit(event.clone(), point(30)).unwrap();
    }
    for index in [2, 3, 4, 5, 1] {
        assert_eq!(
            merge.pop_ready(point(10)).unwrap(),
            Some(events[index].clone())
        );
    }
    assert_eq!(merge.pop_ready(point(10)).unwrap(), None);
    merge.commit(point(10)).unwrap();
    assert_eq!(merge.pop_ready(point(20)).unwrap(), Some(events[0].clone()));
    assert_eq!(merge.pending.capacity(), allocation);
}

#[test]
fn repeat_registration_keeps_history_frontier_and_existing_pending_events() {
    let mut merge = dynamic(3, 4);
    let id = DeviceId(u64::MAX);
    merge.register_source(id).unwrap();
    merge
        .admit(button(id, 20, 10, ButtonState::Down), point(40))
        .unwrap();
    merge.commit(point(10)).unwrap();
    let before = state(&merge);
    merge.register_source(id).unwrap();
    assert_eq!(state(&merge), before);
    assert!(
        matches!(merge.admit(button(id, 19, 11, ButtonState::Up), point(40)), Err(MergeError::SourceTimeRegression { source, .. }) if source == id)
    );
    assert_eq!(state(&merge), before);
    assert!(
        matches!(merge.admit(button(id, 21, 9, ButtonState::Up), point(40)), Err(MergeError::SequenceRegression { source, .. }) if source == id)
    );
    assert_eq!(state(&merge), before);
    merge.register_source(DeviceId(0)).unwrap();
    assert_eq!(merge.committed, before.committed);
    assert_eq!(
        merge
            .sources
            .iter()
            .find(|source| source.device == id)
            .unwrap()
            .last,
        Some((Timestamp::from_nanos(20), 10))
    );
    assert_eq!(
        merge.pop_ready(point(20)).unwrap(),
        Some(before.pending[0].1.clone())
    );
}

#[test]
fn malformed_future_late_and_queue_budget_failures_preserve_dynamic_state() {
    let mut merge = dynamic(2, 2);
    let id = DeviceId(u64::MAX);
    merge.register_source(id).unwrap();
    merge.register_source(DeviceId(1)).unwrap();
    merge.commit(point(10)).unwrap();
    merge
        .admit(button(id, 20, 10, ButtonState::Down), point(40))
        .unwrap();
    let before = state(&merge);
    let mut wrong_domain = button(id, 21, 11, ButtonState::Up);
    wrong_domain.meta_mut().clock_domain = ClockDomainId(11);
    let cases = [
        (
            wrong_domain,
            point(40),
            MergeError::DomainMismatch {
                expected: ClockDomainId(10),
                actual: ClockDomainId(11),
            },
        ),
        (
            button(id, 41, 11, ButtonState::Up),
            point(40),
            MergeError::FutureInput {
                timestamp: Timestamp::from_nanos(41),
                received: Timestamp::from_nanos(40),
            },
        ),
        (
            button(id, 10, 11, ButtonState::Up),
            point(40),
            MergeError::LateInput {
                timestamp: Timestamp::from_nanos(10),
                committed: Timestamp::from_nanos(10),
            },
        ),
        (
            button(id, -1, 11, ButtonState::Up),
            point(40),
            MergeError::BeforeOrigin {
                timestamp: Timestamp::from_nanos(-1),
                origin: Timestamp::from_nanos(0),
            },
        ),
        (
            button(id, 21, 11, ButtonState::Up),
            ClockPoint {
                domain: ClockDomainId(11),
                ..point(40)
            },
            MergeError::DomainMismatch {
                expected: ClockDomainId(10),
                actual: ClockDomainId(11),
            },
        ),
    ];
    for (event, received, error) in cases {
        assert_eq!(merge.admit(event, received), Err(error));
        assert_eq!(state(&merge), before);
    }
    let oversized = PhysicalInputEvent::RawHidReport(RawHidReportEvent {
        meta: EventMeta::new(id, point(21), 11),
        report_id: Some(7),
        data: Vec::with_capacity(MAX_PENDING_BYTES),
    });
    assert_eq!(
        merge.admit(oversized, point(40)),
        Err(MergeError::StorageCapacity)
    );
    assert_eq!(state(&merge), before);
    merge
        .admit(button(id, 21, 11, ButtonState::Up), point(40))
        .unwrap();
    let full = state(&merge);
    assert_eq!(
        merge.admit(button(DeviceId(1), 22, 1, ButtonState::Down), point(40)),
        Err(MergeError::Capacity)
    );
    assert_eq!(state(&merge), full);
}

#[test]
fn dynamically_registered_custom_payload_and_native_provenance_return_unchanged() {
    let mut merge = dynamic(1, 2);
    let id = DeviceId(u64::MAX);
    merge.register_source(id).unwrap();
    let meta = *button(id, 10, 77, ButtonState::Down).meta();
    let mut payload = Vec::with_capacity(257);
    payload.extend_from_slice(&[0, 255, 7, 42]);
    let allocation = payload.capacity();
    let pointer = payload.as_ptr();
    let event = PhysicalInputEvent::Custom(CustomInputEvent {
        meta,
        namespace: VendorNamespaceId(9),
        type_id: 2,
        payload,
    });
    let expected = event.clone();
    merge.admit(event, point(20)).unwrap();
    assert_eq!(merge.payload_bytes, allocation);
    merge.register_source(id).unwrap();
    let returned = merge.pop_ready(point(10)).unwrap().unwrap();
    assert_eq!(returned, expected);
    let PhysicalInputEvent::Custom(returned) = returned else {
        panic!("payload variant changed")
    };
    assert_eq!(returned.payload.capacity(), allocation);
    assert_eq!(returned.payload.as_ptr(), pointer);
    assert_eq!(merge.payload_bytes, 0);
}
