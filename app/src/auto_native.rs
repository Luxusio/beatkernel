//! Native metadata operations for shared preparation on the game owner.
use crate::Result;
use beatkernel_bms_runtime::{
    device_catalog::MAX_DEVICES,
    native_defaults::{
        NativeDefaults, NativeKeyboardCandidate, NativeOutputFormat, NativePreparationDevice,
        NativePreparationResult, PreparationMode, complete,
    },
    settings::SettingsHost,
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use beatkernel_bms_runtime::device_catalog::MAX_DEVICE_TEXT_BYTES;

fn host() -> SettingsHost {
    if cfg!(target_os = "windows") {
        SettingsHost::Windows
    } else if cfg!(target_os = "macos") {
        SettingsHost::Macos
    } else {
        SettingsHost::Linux
    }
}
pub(super) fn syntax_args(args: &[String]) -> Result<Vec<String>> {
    let mut defaults = NativeDefaults::for_validation();
    // The strict macOS parser requires positive numeric device IDs.
    if cfg!(target_os = "macos") {
        defaults.device = "1".into();
    }
    Ok(complete(args, host(), &defaults)?)
}
pub(super) fn prepare(args: &[String]) -> Result<Vec<String>> {
    beatkernel_bms_runtime::native_defaults::prepare(
        args,
        host(),
        PreparationMode::Play,
        &mut NativeDiscovery,
        crate::app::validate_native,
    )
}

#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
pub(super) fn syntax_replay(args: &[String]) -> Result<Vec<String>> {
    let mut defaults = NativeDefaults::for_validation();
    if cfg!(target_os = "macos") {
        defaults.device = "1".into();
    }
    Ok(beatkernel_bms_runtime::native_defaults::replay_args(
        args,
        host(),
        &defaults,
    )?)
}

#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
pub(super) fn prepare_replay(args: &[String]) -> Result<Vec<String>> {
    beatkernel_bms_runtime::native_defaults::prepare(
        args,
        host(),
        PreparationMode::Replay,
        &mut NativeDiscovery,
        crate::app::validate_replay,
    )
}

/// Adapts OS metadata without deciding which omitted options require a query.
struct NativeDiscovery;
impl NativePreparationDevice for NativeDiscovery {
    fn default_output(&mut self) -> NativePreparationResult<String> {
        #[cfg(target_os = "windows")]
        {
            use beatkernel_platform::audio::{AudioDeviceState, AudioOutputBackend};
            let devices = beatkernel_platform::windows::audio::WasapiBackend.devices()?;
            check_count(devices.len())?;
            return Ok(devices
                .into_iter()
                .find(|device| {
                    device.state == AudioDeviceState::Active && device.default_multimedia
                })
                .ok_or("no active OS default multimedia output")?
                .id
                .0);
        }
        #[cfg(target_os = "macos")]
        {
            return Ok(
                beatkernel_platform::macos::audio::CoreAudioStream::default_output_device()?
                    .to_string(),
            );
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        Err("native default output identity query is unavailable on this platform".into())
    }
    fn output_format(&mut self, device: &str) -> NativePreparationResult<NativeOutputFormat> {
        #[cfg(target_os = "windows")]
        {
            use beatkernel_platform::audio::{AudioDeviceId, AudioOutputBackend};
            let format = beatkernel_platform::windows::audio::WasapiBackend
                .mix_format(&AudioDeviceId(device.to_owned()))?;
            return Ok(NativeOutputFormat {
                rate: f64::from(format.sample_rate()),
                channels: u32::from(format.channels()),
                buffer_frames: None,
            });
        }
        #[cfg(target_os = "macos")]
        {
            let id = device.parse::<u32>()?;
            let devices = beatkernel_platform::macos::audio::CoreAudioStream::devices()?;
            check_count(devices.len())?;
            let device = devices
                .into_iter()
                .find(|entry| entry.id == id)
                .ok_or("selected/default audio device has no output")?;
            return Ok(NativeOutputFormat {
                rate: device.nominal_rate,
                channels: device.output_channels,
                buffer_frames: Some(device.buffer_frames),
            });
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            let _ = device;
            Err("native output format query is unavailable on this platform".into())
        }
    }
    fn keyboards(&mut self) -> NativePreparationResult<Vec<NativeKeyboardCandidate>> {
        #[cfg(target_os = "linux")]
        {
            return beatkernel_platform::linux::evdev_keyboard_devices(
                MAX_DEVICES,
                MAX_DEVICE_TEXT_BYTES,
            )?
            .into_iter()
            .enumerate()
            .map(|(index, device)| {
                Ok(NativeKeyboardCandidate {
                    id: device.path,
                    selectable: device.selectable,
                    order: u64::try_from(index)?,
                })
            })
            .collect();
        }
        #[cfg(target_os = "macos")]
        {
            return Ok(beatkernel_platform::macos::input::keyboard_devices(
                MAX_DEVICES,
                MAX_DEVICE_TEXT_BYTES,
            )?
            .into_iter()
            .map(|device| NativeKeyboardCandidate {
                id: device.registry_entry.to_string(),
                selectable: true,
                order: device.registry_entry,
            })
            .collect());
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        Err("automatic keyboard metadata query is unavailable on this platform".into())
    }
    fn asio_format(&mut self, projected: &[String]) -> NativePreparationResult<NativeOutputFormat> {
        #[cfg(target_os = "windows")]
        {
            let format = crate::replay_player::default_asio_format(projected)?;
            return Ok(NativeOutputFormat {
                rate: f64::from(format.sample_rate()),
                channels: u32::from(format.channels()),
                buffer_frames: None,
            });
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = projected;
            Err("ASIO native format query is unavailable on this platform".into())
        }
    }
}
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn check_count(count: usize) -> NativePreparationResult<()> {
    if count > MAX_DEVICES {
        Err("native output metadata exceeds admission limit".into())
    } else {
        Ok(())
    }
}
