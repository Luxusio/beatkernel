//! Native metadata discovery for the settings worker; never opens output streams.

use beatkernel_bms_runtime::device_catalog::{
    DeviceCatalog, DeviceChoice, DeviceRequest, MAX_DEVICES,
};
use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub(super) fn query(request: DeviceRequest) -> Result<DeviceCatalog> {
    let entries = match &request {
        #[cfg(target_os = "windows")]
        DeviceRequest::Wasapi => {
            use beatkernel_platform::audio::{AudioDeviceState, AudioOutputBackend};
            let devices = beatkernel_platform::windows::audio::WasapiBackend.devices()?;
            check_count(devices.len())?;
            devices.into_iter().map(|device| DeviceChoice {
                id: device.id.0,
                label: device.name,
                detail: format!(
                    "State: {:?}; default console: {}; multimedia: {}; communications: {}. Format/mode support requires preparation.",
                    device.state, device.default_console, device.default_multimedia, device.default_communications,
                ),
                selectable: device.state == AudioDeviceState::Active,
            }).collect()
        }
        #[cfg(target_os = "windows")]
        DeviceRequest::Asio { view } => {
            use beatkernel_bms_runtime::device_catalog::{AsioView, MAX_DEVICE_TEXT_BYTES};
            use beatkernel_platform::windows::asio::{
                AsioEnumerationLimits, AsioRegistryView, enumerate_asio_drivers,
            };
            let registry_view = match view {
                AsioView::Native => AsioRegistryView::Native,
                AsioView::Bits32 => AsioRegistryView::Bits32,
                AsioView::Bits64 => AsioRegistryView::Bits64,
            };
            let drivers = enumerate_asio_drivers(
                registry_view,
                AsioEnumerationLimits {
                    max_drivers: MAX_DEVICES,
                    max_value_units: MAX_DEVICE_TEXT_BYTES,
                },
            )?;
            check_count(drivers.len())?;
            drivers.into_iter().map(|driver| DeviceChoice {
                id: driver.id.clsid,
                label: driver.name,
                detail: format!(
                    "Registry view: {:?}; {}. Registration only; driver compatibility/output unverified.",
                    driver.id.view, driver.description.as_deref().unwrap_or("No description"),
                ),
                selectable: true,
            }).collect()
        }
        #[cfg(target_os = "linux")]
        DeviceRequest::Alsa => {
            use beatkernel_bms_runtime::device_catalog::MAX_DEVICE_TEXT_BYTES;
            let devices = beatkernel_platform::linux::alsa_output_devices(
                MAX_DEVICES,
                MAX_DEVICE_TEXT_BYTES,
            )?;
            check_count(devices.len())?;
            devices
                .into_iter()
                .map(|device| DeviceChoice {
                    id: device.name.clone(),
                    label: device.name,
                    detail: format!(
                        "{}; output PCM hint only; availability/format unverified.",
                        device.description.as_deref().unwrap_or("No description")
                    ),
                    selectable: true,
                })
                .collect()
        }
        #[cfg(target_os = "macos")]
        DeviceRequest::Coreaudio => {
            let devices = beatkernel_platform::macos::audio::CoreAudioStream::devices()?;
            check_count(devices.len())?;
            devices.into_iter().map(|device| DeviceChoice {
                id: device.id.to_string(),
                label: device.name.unwrap_or_else(|| format!("Audio device {}", device.id)),
                detail: format!(
                    "UID: {}; nominal rate: {} Hz; output channels: {}; current buffer: {} frames. Exact requested configuration requires preparation.",
                    device.uid.as_deref().unwrap_or("unreported"), device.nominal_rate, device.output_channels, device.buffer_frames,
                ),
                selectable: device.output_channels > 0,
            }).collect()
        }
        #[cfg(target_os = "windows")]
        DeviceRequest::WindowsKeyboard => {
            use beatkernel::time::ClockDomainId;
            use beatkernel_platform::{
                raw_input::RawDeviceKind,
                windows::{clock::QpcClock, input::WindowsInput},
            };
            let mut input = WindowsInput::new(QpcClock::new(ClockDomainId(1))?);
            let devices = input.enumerate_devices()?;
            check_count(devices.len())?;
            devices.into_iter().filter(|device| device.kind == RawDeviceKind::Keyboard).map(|device| DeviceChoice {
                id: device.interface_path.clone(),
                label: device.descriptor.name.unwrap_or_else(|| device.interface_path.clone()),
                detail: format!("Raw Input keyboard; vendor {:?}; product {:?}; exact attachment resolved at play. Other keyboards excluded when selected.", device.descriptor.vendor_id, device.descriptor.product_id),
                selectable: !device.interface_path.is_empty(),
            }).collect()
        }
        #[cfg(target_os = "linux")]
        DeviceRequest::LinuxKeyboard => {
            use beatkernel_bms_runtime::device_catalog::MAX_DEVICE_TEXT_BYTES;
            let devices = beatkernel_platform::linux::evdev_keyboard_devices(
                MAX_DEVICES,
                MAX_DEVICE_TEXT_BYTES,
            )?;
            devices
                .into_iter()
                .map(|device| DeviceChoice {
                    label: device.name.unwrap_or_else(|| device.path.clone()),
                    id: device.path,
                    detail: device.detail,
                    selectable: device.selectable,
                })
                .collect()
        }
        #[cfg(target_os = "macos")]
        DeviceRequest::MacosKeyboard => {
            use beatkernel_bms_runtime::device_catalog::MAX_DEVICE_TEXT_BYTES;
            let devices = beatkernel_platform::macos::input::keyboard_devices(
                MAX_DEVICES,
                MAX_DEVICE_TEXT_BYTES,
            )?;
            devices.into_iter().map(|device| DeviceChoice {
                id: device.registry_entry.to_string(),
                label: device.name.unwrap_or_else(|| format!("Keyboard {}", device.registry_entry)),
                detail: format!("IORegistry {}; vendor {:?}; product {:?}; transport {}. Acquisition access verified at play.", device.registry_entry, device.vendor_id, device.product_id, device.transport.as_deref().unwrap_or("unreported")),
                selectable: true,
            }).collect()
        }
        _ => return Err("requested audio device backend is unavailable on this platform".into()),
    };
    Ok(DeviceCatalog::new(request, entries)?)
}

fn check_count(count: usize) -> Result<()> {
    if count > MAX_DEVICES {
        return Err(format!("native audio discovery exceeds {MAX_DEVICES} devices").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_foreign_backend_without_native_discovery() {
        #[cfg(target_os = "windows")]
        let request = DeviceRequest::Alsa;
        #[cfg(not(target_os = "windows"))]
        let request = DeviceRequest::Wasapi;
        assert!(
            query(request)
                .err()
                .expect("foreign backend must fail before discovery")
                .to_string()
                .contains("unavailable on this platform")
        );
        assert!(check_count(MAX_DEVICES).is_ok());
        assert!(check_count(MAX_DEVICES + 1).is_err());
    }
}
