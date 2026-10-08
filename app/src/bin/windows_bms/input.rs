//! Raw Input acquisition owned solely by the collector thread.

pub(super) fn finish_raw_input<T, E>(
    decoded: std::result::Result<T, E>,
    foreground: bool,
    cleanup: impl FnOnce(),
    admit: impl FnOnce(T) -> std::result::Result<(), E>,
) -> std::result::Result<(), E> {
    if foreground {
        cleanup();
    }
    admit(decoded?)
}

#[cfg(target_os = "windows")]
mod native {
    use super::finish_raw_input;
    use crate::{
        native::{AcquisitionWindow, HOST},
        Result,
    };
    use beatkernel::input::DeviceId;
    use beatkernel_bms_runtime::{
        local_players::PlayerId,
        native_input::{
            CollectorConfig, InputPublisher, NativeInputCollector, NativeInputSource, SourceDrain,
        },
    };
    use beatkernel_platform::{
        raw_input::RawDeviceKind,
        windows::{clock::QpcClock, input::WindowsInput},
    };
    use std::{
        ptr,
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc,
        },
        time::Duration,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, DispatchMessageW, PeekMessageW, TranslateMessage, GIDC_ARRIVAL,
        GIDC_REMOVAL, MSG, PM_REMOVE, WM_CLOSE, WM_INPUT, WM_INPUT_DEVICE_CHANGE, WM_QUIT,
    };

    pub(crate) enum Selection {
        Solo(Option<String>),
        Local(Vec<(PlayerId, String)>),
    }
    pub(crate) struct CollectorOwner {
        pub(crate) collector: NativeInputCollector,
        active: Arc<AtomicBool>,
    }
    impl CollectorOwner {
        pub(crate) fn open(
            clock: QpcClock,
            selection: Selection,
        ) -> Result<(Self, Vec<(DeviceId, usize)>)> {
            let visible = !beatkernel_bms_runtime::player::attached();
            let active = Arc::new(AtomicBool::new(false));
            let worker_active = active.clone();
            let (metadata_tx, metadata_rx) = mpsc::sync_channel(1);
            let mut collector = NativeInputCollector::spawn(
                CollectorConfig {
                    domain: HOST,
                    entries: 4096,
                    bytes: 8 * 1024 * 1024,
                    max_payload_bytes: 1024 * 1024,
                    service_quantum: 256,
                    idle_wait: Duration::from_millis(1),
                },
                move || {
                    let make = || -> Result<RawSource> {
                        let acquisition = AcquisitionWindow::new(visible)?;
                        let mut input = WindowsInput::new(clock);
                        let devices = input.enumerate_devices()?;
                        let attached: Vec<_> = devices
                            .iter()
                            .filter(|d| d.kind == RawDeviceKind::Keyboard)
                            .map(|d| {
                                (
                                    d.interface_path.as_str(),
                                    d.descriptor.runtime_id.0,
                                    d.handle,
                                )
                            })
                            .collect();
                        let selected = match selection {
                            Selection::Solo(path) => {
                                crate::selected_keyboard(path.as_deref(), attached.iter().copied())?
                                    .map(|(id, handle)| (DeviceId(id), handle))
                                    .into_iter()
                                    .collect()
                            }
                            Selection::Local(requested) => {
                                crate::local_native::resolve_keyboards(&requested, &attached)?
                            }
                        };
                        metadata_tx
                            .try_send(selected.clone())
                            .map_err(|_| "input selection metadata receiver lost")?;
                        Ok(RawSource {
                            acquisition,
                            input,
                            selected,
                            clock,
                            active: worker_active,
                        })
                    };
                    make().map_err(|e| e.to_string())
                },
            )?;
            collector.wait_ready()?;
            let selected = metadata_rx
                .try_recv()
                .map_err(|_| "input selection metadata absent after readiness")?;
            Ok((Self { collector, active }, selected))
        }
        pub(crate) fn activate(&self) {
            self.active.store(true, Ordering::Release);
            self.collector.wake();
        }
        pub(crate) fn stop_and_join(&mut self) -> Result<()> {
            Ok(self.collector.stop_and_join()?)
        }
    }

    struct RawSource {
        acquisition: AcquisitionWindow,
        input: WindowsInput,
        selected: Vec<(DeviceId, usize)>,
        clock: QpcClock,
        active: Arc<AtomicBool>,
    }
    impl NativeInputSource for RawSource {
        fn service(
            &mut self,
            sink: &mut InputPublisher<'_>,
            quantum: usize,
        ) -> std::result::Result<SourceDrain, String> {
            if !self.active.load(Ordering::Acquire) {
                return Ok(SourceDrain {
                    idle: true,
                    ..Default::default()
                });
            }
            // The observation precedes draining; later consumer receipt cannot widen it.
            let cut = self.clock.sample().map_err(|e| e.to_string())?.normalized;
            let mut message: MSG = unsafe { std::mem::zeroed() };
            for _ in 0..quantum {
                // SAFETY: the window and message queue belong to this worker.
                if unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } == 0 {
                    return Ok(SourceDrain {
                        completed_through: Some(cut),
                        idle: true,
                        closed: false,
                    });
                }
                if message.message == WM_QUIT || message.message == WM_CLOSE {
                    return Ok(SourceDrain {
                        closed: true,
                        ..Default::default()
                    });
                }
                if message.hwnd == self.acquisition.hwnd() && message.message == WM_INPUT {
                    let decoded = self
                        .input
                        .read_raw_input(message.lParam as usize, Some(message.time))
                        .map_err(|e| e.to_string());
                    finish_raw_input(
                        decoded,
                        message.wParam & 0xff == 0,
                        || unsafe {
                            DefWindowProcW(
                                message.hwnd,
                                message.message,
                                message.wParam,
                                message.lParam,
                            );
                        },
                        |acquired| {
                            for event in acquired.input.events {
                                if self.selected.is_empty()
                                    || self
                                        .selected
                                        .iter()
                                        .any(|(id, _)| event.meta().source == *id)
                                {
                                    sink.publish(event).map_err(|e| e.to_string())?;
                                }
                            }
                            Ok(())
                        },
                    )?;
                    continue;
                }
                if message.hwnd == self.acquisition.hwnd()
                    && message.message == WM_INPUT_DEVICE_CHANGE
                {
                    match message.wParam as u32 {
                        GIDC_ARRIVAL => {
                            self.input
                                .attach_device(message.lParam as usize)
                                .map_err(|e| e.to_string())?;
                        }
                        GIDC_REMOVAL => {
                            if self
                                .selected
                                .iter()
                                .any(|(_, handle)| *handle == message.lParam as usize)
                            {
                                return Err("selected keyboard detached during acquisition".into());
                            }
                            self.input.remove_device(message.lParam as usize);
                        }
                        _ => {}
                    }
                }
                // SAFETY: native queue message, dispatched on its owning thread.
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            Ok(SourceDrain::default())
        }
        fn close(&mut self) -> std::result::Result<(), String> {
            self.acquisition
                .registration
                .close()
                .map_err(|e| e.to_string())
        }
    }
}
#[cfg(target_os = "windows")]
pub(crate) use native::{CollectorOwner, Selection};

#[cfg(test)]
#[path = "input_fixtures.rs"]
mod fixtures;
