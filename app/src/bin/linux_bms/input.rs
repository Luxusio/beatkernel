//! Collector-owned evdev handles and conservative all-device drain evidence.
use super::native::HOST;
use beatkernel::{
    input::{DeviceDescriptor, DeviceId},
    time::ClockPoint,
};
use beatkernel_bms_runtime::native_input::{
    CollectorConfig, InputPublisher, NativeInputCollector, NativeInputSource, SourceDrain,
};
use beatkernel_platform::linux::{EvdevDevice, EvdevItem, LinuxInputCounters, MonotonicClock};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender},
    time::Duration,
};

pub(super) trait EvdevRead {
    fn read_next(&mut self) -> Result<EvdevItem, String>;
}
impl EvdevRead for EvdevDevice {
    fn read_next(&mut self) -> Result<EvdevItem, String> {
        EvdevDevice::read_next(self).map_err(|e| e.to_string())
    }
}
pub(super) trait DrainClock {
    fn now(&mut self) -> Result<ClockPoint, String>;
}
impl DrainClock for MonotonicClock {
    fn now(&mut self) -> Result<ClockPoint, String> {
        MonotonicClock::now(*self).map_err(|e| e.to_string())
    }
}
pub(super) struct EvdevDrain<D, C> {
    devices: Vec<D>,
    clock: C,
    pending: Vec<bool>,
    cursor: usize,
    cut: Option<ClockPoint>,
}
impl<D, C> EvdevDrain<D, C> {
    pub(super) fn new(devices: Vec<D>, clock: C) -> Result<Self, String> {
        if devices.is_empty() || devices.len() > 64 {
            return Err("invalid local evdev ownership".into());
        }
        Ok(Self {
            pending: vec![true; devices.len()],
            devices,
            clock,
            cursor: 0,
            cut: None,
        })
    }
}
impl<D: EvdevRead, C: DrainClock> NativeInputSource for EvdevDrain<D, C> {
    fn service(
        &mut self,
        sink: &mut InputPublisher<'_>,
        quantum: usize,
    ) -> Result<SourceDrain, String> {
        if self.cut.is_none() {
            self.cut = Some(self.clock.now()?);
            self.pending.fill(true);
        }
        for _ in 0..quantum {
            // Continue the same bounded sweep across service calls. A noisy
            // device cannot prevent another selected source from being polled.
            while !self.pending[self.cursor] {
                self.cursor = (self.cursor + 1) % self.devices.len();
            }
            let index = self.cursor;
            self.cursor = (index + 1) % self.devices.len();
            match self.devices[index].read_next()? {
                EvdevItem::WouldBlock => self.pending[index] = false,
                EvdevItem::Ignored => {}
                EvdevItem::Event(event) => sink.publish(event).map_err(|e| e.to_string())?,
                EvdevItem::Dropped | EvdevItem::Resync(_) => {
                    return Err("evdev loss/resync; cleanup and restart required".into())
                }
            }
            if self.pending.iter().all(|pending| !pending) {
                return Ok(SourceDrain {
                    completed_through: self.cut.take(),
                    idle: true,
                    closed: false,
                });
            }
        }
        Ok(SourceDrain::default())
    }
    fn close(&mut self) -> Result<(), String> {
        self.devices.clear();
        Ok(())
    }
}

struct NativeEvdevInput {
    drain: EvdevDrain<EvdevDevice, MonotonicClock>,
    final_counters: SyncSender<Vec<LinuxInputCounters>>,
}
impl NativeInputSource for NativeEvdevInput {
    fn service(
        &mut self,
        sink: &mut InputPublisher<'_>,
        quantum: usize,
    ) -> Result<SourceDrain, String> {
        self.drain.service(sink, quantum)
    }
    fn close(&mut self) -> Result<(), String> {
        let counters = self
            .drain
            .devices
            .iter()
            .map(EvdevDevice::counters)
            .collect();
        let _ = self.final_counters.try_send(counters);
        self.drain.close()
    }
}
pub(super) fn open(
    paths: Vec<PathBuf>,
) -> super::Result<(
    NativeInputCollector,
    Vec<DeviceDescriptor>,
    Receiver<Vec<LinuxInputCounters>>,
)> {
    let (metadata_sender, metadata) = mpsc::sync_channel(1);
    let (final_counters, counters) = mpsc::sync_channel(1);
    let mut collector = NativeInputCollector::spawn(
        CollectorConfig {
            domain: HOST,
            entries: 16384,
            bytes: 16 * 1024 * 1024,
            max_payload_bytes: 4096,
            service_quantum: 256,
            idle_wait: Duration::from_millis(1),
        },
        move || {
            let mut devices = Vec::with_capacity(paths.len());
            let mut native_numbers = HashSet::new();
            for (index, path) in paths.iter().enumerate() {
                let device = EvdevDevice::open(path, DeviceId((index + 1) as u64), HOST)
                    .map_err(|e| e.to_string())?;
                if !native_numbers.insert(device.native_device_number().map_err(|e| e.to_string())?)
                {
                    return Err("local input paths alias the same physical character device".into());
                }
                devices.push(device);
            }
            let descriptors = devices.iter().map(|d| d.descriptor().clone()).collect();
            let drain = EvdevDrain::new(devices, MonotonicClock::new(HOST))?;
            metadata_sender
                .try_send(descriptors)
                .map_err(|e| e.to_string())?;
            Ok(NativeEvdevInput {
                drain,
                final_counters,
            })
        },
    )?;
    collector.wait_ready()?;
    Ok((collector, metadata.try_recv()?, counters))
}

#[cfg(test)]
#[path = "input_fixtures.rs"]
mod fixtures;
