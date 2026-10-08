//! Worker-owned IOHID acquisition and actual runloop completion evidence.
use beatkernel::{
    input::{DeviceId, PhysicalInputEvent},
    time::ClockPoint,
};
use beatkernel_bms_runtime::native_input::{InputPublisher, NativeInputSource, SourceDrain};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pump {
    IdleAt(ClockPoint),
    Handled,
    Stopped,
}
/// Static seam shared by real IOHID acquisition and deterministic adapter fixtures.
pub(super) trait Acquisition {
    fn poll(&mut self) -> Result<Pump, String>;
    fn pop(&mut self) -> Option<PhysicalInputEvent>;
    fn health(&self) -> Result<(), String>;
    fn close(&mut self) -> Result<(), String>;
}
pub(super) struct Source<A> {
    pub(super) acquisition: A,
    pub(super) selected: Vec<DeviceId>,
    pub(super) active: Arc<AtomicBool>,
}
impl<A: Acquisition> Source<A> {
    fn drain(
        &mut self,
        quantum: usize,
        mut publish: impl FnMut(PhysicalInputEvent) -> Result<(), String>,
    ) -> Result<SourceDrain, String> {
        if !self.active.load(Ordering::Acquire) {
            return Ok(SourceDrain {
                idle: true,
                ..Default::default()
            });
        }
        self.acquisition.health()?;
        let mut idle = None;
        // Both ignored events and native pumping consume the finite budget.
        for _ in 0..quantum {
            if let Some(event) = self.acquisition.pop() {
                if self.selected.contains(&event.meta().source) {
                    publish(event)?;
                }
                continue;
            }
            if let Some(cut) = idle {
                return Ok(SourceDrain {
                    completed_through: Some(cut),
                    idle: true,
                    closed: false,
                });
            }
            match self.acquisition.poll()? {
                Pump::IdleAt(cut) => idle = Some(cut),
                Pump::Handled => {}
                Pump::Stopped => {
                    return Err("IOHID owner runloop stopped/finished; restart required".into())
                }
            }
            self.acquisition.health()?;
        }
        // At budget exhaustion, even an idle pump may have produced callback
        // values; a later empty pop must confirm that all covered values passed.
        Ok(SourceDrain::default())
    }
}
impl<A: Acquisition> NativeInputSource for Source<A> {
    fn service(
        &mut self,
        sink: &mut InputPublisher<'_>,
        quantum: usize,
    ) -> Result<SourceDrain, String> {
        self.drain(quantum, |event| {
            sink.publish(event).map_err(|e| e.to_string())
        })
    }
    fn close(&mut self) -> Result<(), String> {
        self.acquisition.close()
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use beatkernel_bms_runtime::native_input::{CollectorConfig, NativeInputCollector};
    use beatkernel_platform::macos::{
        clock::MachClock,
        input::{HidDevice, HidInput, HidPollCompletion},
    };
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    struct Native {
        input: HidInput,
        registries: Vec<u64>,
        selected: Vec<DeviceId>,
    }
    impl Acquisition for Native {
        fn poll(&mut self) -> Result<Pump, String> {
            // Manager pumping services attachments only; scalar values use checked queues.
            match self
                .input
                .poll_completion(Duration::ZERO)
                .map_err(|e| e.to_string())?
            {
                HidPollCompletion::Stopped | HidPollCompletion::Finished => {
                    return Ok(Pump::Stopped)
                }
                HidPollCompletion::TimedOut | HidPollCompletion::HandledSource => {}
            }
            match self.input.poll_queued(1).map_err(|e| e.to_string())? {
                Some(cut) => Ok(Pump::IdleAt(cut)),
                None => Ok(Pump::Handled),
            }
        }
        fn pop(&mut self) -> Option<PhysicalInputEvent> {
            self.input.pop().map(|v| v.event)
        }
        fn health(&self) -> Result<(), String> {
            let c = self.input.counters();
            if c.queue_full != 0
                || c.unsupported != 0
                || c.reports_queue_full != 0
                || c.reports_oversized != 0
                || c.reports_invalid != 0
                || c.reports_allocation_failed != 0
                || c.reports_timestamp_failed != 0
            {
                return Err(format!("IOHID acquisition loss; restart required: {c:?}"));
            }
            for (&registry, &selected) in self.registries.iter().zip(&self.selected) {
                let candidates = self.input.registry_candidates(registry);
                if candidates != [Some((selected, true)), None] {
                    return Err(
                        "selected IORegistry attachment removed/changed/ambiguous; no retarget"
                            .into(),
                    );
                }
            }
            Ok(())
        }
        fn close(&mut self) -> Result<(), String> {
            self.input.close().map_err(|e| e.to_string())
        }
    }
    pub(crate) struct Collector {
        pub(crate) worker: NativeInputCollector,
        active: Arc<AtomicBool>,
    }
    impl Collector {
        pub(crate) fn activate(&self) {
            self.active.store(true, Ordering::Release);
            self.worker.wake();
        }
        pub(crate) fn status(
            &mut self,
        ) -> Result<bool, beatkernel_bms_runtime::native_input::CollectorError> {
            self.worker.status()
        }
        pub(crate) fn close(
            &mut self,
        ) -> Result<(), beatkernel_bms_runtime::native_input::CollectorError> {
            self.worker.stop_and_join()
        }
    }
    pub(crate) fn open(
        clock: MachClock,
        registries: Vec<u64>,
        capacity: usize,
    ) -> super::super::Result<(Collector, Vec<HidDevice>)> {
        let (tx, rx) = mpsc::sync_channel(1);
        let active = Arc::new(AtomicBool::new(false));
        let enabled = active.clone();
        let mut worker = NativeInputCollector::spawn(
            CollectorConfig {
                domain: clock.sample()?.normalized.domain,
                entries: 65536,
                bytes: 32 * 1024 * 1024,
                max_payload_bytes: 1024,
                service_quantum: 256,
                idle_wait: Duration::from_millis(1),
            },
            move || {
                // The !Send native manager and its callback state never leave this worker.
                let mut input = HidInput::open_queued(clock, DeviceId(1), capacity)
                    .map_err(|e| e.to_string())?;
                let selection = (|| {
                    let deadline = Instant::now() + Duration::from_secs(2);
                    loop {
                        let devices = input.devices();
                        let mut selected = Vec::with_capacity(registries.len());
                        for registry in &registries {
                            let mut matching = devices
                                .iter()
                                .filter(|d| d.registry_entry == Some(*registry));
                            if let Some(device) = matching.next() {
                                if matching.next().is_some()
                                    || !device.descriptor.capabilities.button
                                {
                                    return Err(
                                        "ambiguous/non-button IORegistry selection".to_string()
                                    );
                                }
                                if selected.iter().any(|d: &HidDevice| {
                                    d.descriptor.runtime_id == device.descriptor.runtime_id
                                }) {
                                    return Err(
                                        "IORegistry selection aliases another player".to_string()
                                    );
                                }
                                selected.push(device.clone());
                            }
                        }
                        if selected.len() == registries.len() {
                            return Ok(selected);
                        }
                        if Instant::now() >= deadline {
                            return Err(
                                "IORegistry selection unavailable within two seconds".into()
                            );
                        }
                        input
                            .poll(Duration::from_millis(1))
                            .map_err(|e| e.to_string())?;
                    }
                })();
                let devices = match selection {
                    Ok(devices) => devices,
                    Err(error) => {
                        return match input.close() {
                            Ok(()) => Err(error),
                            Err(close) => Err(format!("{error}; IOHID close: {close}")),
                        }
                    }
                };
                let selected: Vec<_> = devices.iter().map(|d| d.descriptor.runtime_id).collect();
                if let Err(error) = input.select_queued_devices(&selected) {
                    let close = input.close();
                    return Err(format!(
                        "checked IOHID queue selection: {error}; close={close:?}"
                    ));
                }
                if tx.try_send(devices).is_err() {
                    let close = input.close();
                    return Err(format!(
                        "IOHID selection receiver unavailable; close={close:?}"
                    ));
                }
                Ok(Source {
                    acquisition: Native {
                        input,
                        registries,
                        selected: selected.clone(),
                    },
                    selected,
                    active: enabled,
                })
            },
        )?;
        if let Err(error) = worker.wait_ready() {
            let _ = worker.stop_and_join();
            return Err(error.into());
        }
        let metadata = match rx.try_recv() {
            Ok(value) => value,
            Err(error) => {
                let close = worker.stop_and_join();
                return Err(format!("IOHID selection metadata: {error}; cleanup={close:?}").into());
            }
        };
        Ok((Collector { worker, active }, metadata))
    }
}
#[cfg(target_os = "macos")]
pub(super) use native::{open, Collector};
#[cfg(test)]
#[path = "input_fixtures.rs"]
mod input_fixtures;
