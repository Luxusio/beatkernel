//! Resolve app defaults on the native game owner, never on the UI/audio callback.
use crate::Result;
use beatkernel_bms_runtime::native_defaults::{NativeDefaults, complete};
use beatkernel_bms_runtime::settings::SettingsHost;

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
    // The macOS strict parser requires positive numeric device IDs.
    if cfg!(target_os = "macos") {
        defaults.device = "1".into();
    }
    Ok(complete(args, host(), &defaults)?)
}
pub(super) fn prepare(args: &[String]) -> Result<Vec<String>> {
    // Parser-only defaults validate syntax before any discovery, never device availability.
    crate::app::validate_native(args)?;
    let get = |flag: &str| {
        args.chunks_exact(2)
            .find(|p| p[0] == flag)
            .map(|p| p[1].as_str())
    };
    let mut defaults = NativeDefaults::for_validation();
    #[cfg(target_os = "windows")]
    if get("--device").is_none() {
        if get("--backend") == Some("asio") {
            return Err(
                "ASIO has no OS default driver; select its advanced driver configuration".into(),
            );
        }
        use beatkernel_platform::audio::{AudioDeviceState, AudioOutputBackend};
        let devices = beatkernel_platform::windows::audio::WasapiBackend.devices()?;
        if devices.len() > 1024 {
            return Err("default output metadata exceeds admission limit".into());
        }
        defaults.device = devices
            .into_iter()
            .find(|d| d.state == AudioDeviceState::Active && d.default_multimedia)
            .ok_or("no active OS default multimedia output")?
            .id
            .0;
    }
    #[cfg(target_os = "linux")]
    if get("--evdev").is_none() && get("--local-input").is_none() && get("--local-player").is_none()
    {
        defaults.keyboard = beatkernel_platform::linux::evdev_keyboard_devices(1024, 4096)?
            .into_iter()
            .find(|d| d.selectable)
            .ok_or("no readable keyboard found; check native input permissions")?
            .path;
    }
    #[cfg(target_os = "macos")]
    {
        use beatkernel_platform::macos::{audio::CoreAudioStream, input::keyboard_devices};
        if get("--device").is_none()
            || get("--rate").is_none()
            || get("--channels").is_none()
            || get("--buffer-frames").is_none()
        {
            let id = match get("--device") {
                Some(id) => id.parse::<u32>()?,
                None => CoreAudioStream::default_output_device()?,
            };
            let devices = CoreAudioStream::devices()?;
            if devices.len() > 1024 {
                return Err("output metadata exceeds admission limit".into());
            }
            let device = devices
                .into_iter()
                .find(|d| d.id == id)
                .ok_or("selected/default audio device has no output")?;
            if !device.nominal_rate.is_finite()
                || device.nominal_rate <= 0.0
                || device.nominal_rate.fract() != 0.0
                || device.nominal_rate > f64::from(u32::MAX)
            {
                return Err("default audio rate is not an integer supported rate".into());
            }
            defaults.device = id.to_string();
            defaults.rate = device.nominal_rate as u32;
            defaults.channels = u16::try_from(device.output_channels.min(2))?;
            defaults.buffer_frames = device.buffer_frames;
        }
        if get("--keyboard-registry").is_none() && get("--local-player").is_none() {
            let mut devices = keyboard_devices(1024, 4096)?;
            devices.sort_by_key(|d| d.registry_entry);
            defaults.keyboard = devices
                .first()
                .ok_or("no keyboard found")?
                .registry_entry
                .to_string();
        }
    }
    Ok(complete(args, host(), &defaults)?)
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

/// Resolve output defaults only: watching never queries/acquires keyboards.
#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
pub(super) fn prepare_replay(args: &[String]) -> Result<Vec<String>> {
    crate::app::validate_replay(args)?;
    let get = |flag: &str| {
        args.chunks_exact(2)
            .find(|p| p[0] == flag)
            .map(|p| p[1].as_str())
    };
    let mut defaults = NativeDefaults::for_validation();
    #[cfg(target_os = "windows")]
    {
        use beatkernel_platform::audio::{AudioDeviceId, AudioDeviceState, AudioOutputBackend};
        let asio = get("--backend") == Some("asio");
        if get("--device").is_none() {
            if asio {
                return Err("ASIO requires an explicit driver configuration".into());
            }
            let devices = beatkernel_platform::windows::audio::WasapiBackend.devices()?;
            if devices.len() > 1024 {
                return Err("output metadata exceeds admission limit".into());
            }
            defaults.device = devices
                .into_iter()
                .find(|d| d.state == AudioDeviceState::Active && d.default_multimedia)
                .ok_or("no active OS default multimedia output")?
                .id
                .0;
        }
        if !asio && (get("--rate").is_none() || get("--channels").is_none()) {
            let id = AudioDeviceId(get("--device").unwrap_or(&defaults.device).to_owned());
            let format = beatkernel_platform::windows::audio::WasapiBackend.mix_format(&id)?;
            defaults.rate = format.sample_rate();
            defaults.channels = format.channels();
        }
    }
    #[cfg(target_os = "macos")]
    {
        use beatkernel_platform::macos::audio::CoreAudioStream;
        if get("--device").is_none()
            || get("--rate").is_none()
            || get("--channels").is_none()
            || get("--buffer-frames").is_none()
        {
            let id = match get("--device") {
                Some(id) => id.parse::<u32>()?,
                None => CoreAudioStream::default_output_device()?,
            };
            let devices = CoreAudioStream::devices()?;
            if devices.len() > 1024 {
                return Err("output metadata exceeds admission limit".into());
            }
            let device = devices
                .into_iter()
                .find(|d| d.id == id)
                .ok_or("selected/default audio device has no output")?;
            if !device.nominal_rate.is_finite()
                || device.nominal_rate <= 0.0
                || device.nominal_rate.fract() != 0.0
                || device.nominal_rate > f64::from(u32::MAX)
            {
                return Err("default audio rate is not an integer supported rate".into());
            }
            defaults.device = id.to_string();
            defaults.rate = device.nominal_rate as u32;
            defaults.channels = u16::try_from(device.output_channels.min(2))?;
            defaults.buffer_frames = device.buffer_frames;
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = (&get, &mut defaults);
    Ok(beatkernel_bms_runtime::native_defaults::replay_args(
        args,
        host(),
        &defaults,
    )?)
}
