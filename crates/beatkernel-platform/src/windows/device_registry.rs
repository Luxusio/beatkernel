//! Single-owner native-handle registry with never-reused runtime identities.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashSet};

use beatkernel::input::{DeviceCapabilities, DeviceDescriptor, DeviceId, PhysicalControlId};

use crate::raw_input::{RawDeviceInfo, RawDeviceKind, RawInputError, MAX_RAW_INPUT_DEVICES};

#[derive(Debug)]
pub(crate) struct DeviceRecord {
    pub(crate) descriptor: DeviceDescriptor,
    pub(crate) kind: RawDeviceKind,
    pub(crate) sequence: u64,
    pub(crate) held: HashSet<PhysicalControlId>,
    pub(crate) pause_header: bool,
}

#[derive(Debug)]
pub(crate) struct DeviceRegistry {
    pub(crate) records: BTreeMap<u64, DeviceRecord>,
    pub(crate) next_id: Option<u64>,
}

impl DeviceRegistry {
    pub(crate) fn new() -> Self {
        Self {
            records: BTreeMap::new(),
            next_id: Some(1),
        }
    }

    pub(crate) fn register(
        &mut self,
        handle: u64,
        info: RawDeviceInfo,
    ) -> Result<DeviceId, RawInputError> {
        if handle == 0 {
            return Err(RawInputError::InvalidDeviceHandle);
        }
        if let Some(record) = self.records.get(&handle) {
            return if record.kind == info.kind {
                Ok(record.descriptor.runtime_id)
            } else {
                Err(RawInputError::DeviceKindMismatch)
            };
        }
        if self.records.len() >= MAX_RAW_INPUT_DEVICES {
            return Err(RawInputError::DeviceLimit);
        }
        let value = self.next_id.ok_or(RawInputError::DeviceIdExhausted)?;
        let id = DeviceId(value);
        self.records.insert(
            handle,
            DeviceRecord {
                descriptor: DeviceDescriptor {
                    runtime_id: id,
                    vendor_id: info.vendor_id,
                    product_id: info.product_id,
                    serial: info.serial,
                    name: info.name,
                    transport: info.transport,
                    capabilities: DeviceCapabilities {
                        button: info.kind == RawDeviceKind::Keyboard,
                        raw_hid: info.kind == RawDeviceKind::Hid,
                        ..DeviceCapabilities::default()
                    },
                },
                kind: info.kind,
                sequence: 0,
                held: HashSet::new(),
                pause_header: false,
            },
        );
        self.next_id = value.checked_add(1);
        Ok(id)
    }
}
