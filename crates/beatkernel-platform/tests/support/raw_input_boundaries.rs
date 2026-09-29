use super::*;

fn key_bytes(handle: u64, make: u16, flags: u16, vkey: u16) -> Vec<u8> {
    let mut bytes = 1u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&handle.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    for value in [make, flags, 0, vkey] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 8]);
    bytes
}

#[test]
fn final_runtime_id_is_valid_and_exhaustion_never_reuses_it() {
    let mut processor = RawInputProcessor::new(ClockDomainId(1));
    processor.registry.next_id = Some(u64::MAX);
    let info = RawDeviceInfo::new(RawDeviceKind::Keyboard);
    assert_eq!(
        processor.register_device(1, info.clone()),
        Ok(DeviceId(u64::MAX))
    );
    assert_eq!(
        processor.register_device(2, info.clone()),
        Err(RawInputError::DeviceIdExhausted)
    );
    assert_eq!(processor.devices().count(), 1);
    assert_eq!(
        processor.register_device(1, info.clone()),
        Ok(DeviceId(u64::MAX))
    );
    processor.unregister_device(1);
    assert_eq!(
        processor.register_device(1, info),
        Err(RawInputError::DeviceIdExhausted)
    );
    assert_eq!(processor.devices().count(), 0);
}

#[test]
fn sequence_exhaustion_preserves_descriptor_held_and_pending_state() {
    let mapping = QpcClockMapping::new(1_000_000_000, 0, ClockDomainId(1)).unwrap();
    let mut processor = RawInputProcessor::new(ClockDomainId(1));
    let id = processor
        .register_device(1, RawDeviceInfo::new(RawDeviceKind::Keyboard))
        .unwrap();
    let down = key_bytes(1, 0x1e, 0, 0x41);
    processor
        .process(
            &RawInputPacket::parse(&down, RawInputLayout::Win64).unwrap(),
            mapping.point(1).unwrap(),
            &mapping,
        )
        .unwrap();
    let record = processor.registry.records.get_mut(&1).unwrap();
    record.sequence = u64::MAX;
    record.pause_header = true;
    let saved = record.descriptor.clone();
    let held = record.held.clone();
    let up = key_bytes(1, 0x1e, 1, 0x41);
    assert_eq!(
        processor.process(
            &RawInputPacket::parse(&up, RawInputLayout::Win64).unwrap(),
            mapping.point(2).unwrap(),
            &mapping
        ),
        Err(RawInputError::SequenceExhausted(id))
    );
    let record = &processor.registry.records[&1];
    assert_eq!(record.sequence, u64::MAX);
    assert_eq!(record.descriptor, saved);
    assert_eq!(record.held, held);
    assert!(record.pause_header);
}

#[test]
fn active_device_limit_rejects_atomically_and_retirement_frees_capacity_only() {
    let mut processor = RawInputProcessor::new(ClockDomainId(1));
    let info = RawDeviceInfo::new(RawDeviceKind::Hid);
    for handle in 1..=MAX_RAW_INPUT_DEVICES as u64 {
        processor.register_device(handle, info.clone()).unwrap();
    }
    let next = processor.registry.next_id;
    assert_eq!(
        processor.register_device(9000, info.clone()),
        Err(RawInputError::DeviceLimit)
    );
    assert_eq!(processor.registry.next_id, next);
    assert_eq!(processor.devices().count(), MAX_RAW_INPUT_DEVICES);
    processor.unregister_device(1);
    assert_eq!(
        processor.register_device(9000, info).unwrap(),
        DeviceId(MAX_RAW_INPUT_DEVICES as u64 + 1)
    );
    assert_eq!(processor.devices().count(), MAX_RAW_INPUT_DEVICES);
}
