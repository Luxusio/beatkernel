//! App-level solo defaults; platform requests stay exact after preparation.
use crate::settings::{NativeSettings, SettingsHost};

#[derive(Clone, Debug)]
pub struct NativeDefaults {
    pub device: String,
    pub keyboard: String,
    pub rate: u32,
    pub channels: u16,
    pub buffer_frames: u32,
}
impl NativeDefaults {
    /// Parser-only values: never passed to native execution or stored in a profile.
    pub fn for_validation() -> Self {
        Self {
            device: "{00000000-0000-0000-0000-000000000001}".into(),
            keyboard: "1".into(),
            rate: 48000,
            channels: 2,
            buffer_frames: 1024,
        }
    }
}
/// Add omitted defaults only; explicitly supplied values retain normal strict errors.
pub fn complete(
    args: &[String],
    host: SettingsHost,
    defaults: &NativeDefaults,
) -> Result<Vec<String>, String> {
    NativeSettings::from_args(args, host)?;
    let mut result = args.to_vec();
    let get = |flag: &str| {
        args.chunks_exact(2)
            .find(|pair| pair[0] == flag)
            .map(|pair| pair[1].as_str())
    };
    let mut add = |flag: &str, value: String| {
        if get(flag).is_none() {
            result.extend([flag.to_owned(), value]);
        }
    };
    match host {
        SettingsHost::Windows => {
            add("--device", defaults.device.clone());
            if get("--backend") != Some("asio") {
                add("--mode", "shared".into());
            }
        }
        SettingsHost::Linux => {
            add("--alsa", "default".into());
            add("--evdev", defaults.keyboard.clone());
            add("--rate", defaults.rate.to_string());
            add("--channels", defaults.channels.to_string());
            add("--period-frames", "256".into());
            add("--buffer-frames", defaults.buffer_frames.to_string());
        }
        SettingsHost::Macos => {
            add("--device", defaults.device.clone());
            add("--keyboard-registry", defaults.keyboard.clone());
            add("--rate", defaults.rate.to_string());
            add("--channels", defaults.channels.to_string());
            add("--buffer-frames", defaults.buffer_frames.to_string());
        }
    }
    add("--channel-policy", "mono-stereo".into());
    NativeSettings::from_args(&result, host)?;
    Ok(result)
}
#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn automatic_defaults_preserve_explicit_overrides_and_leave_draft_unmodified() {
        let args = [
            "--chart",
            "song.bms",
            "--evdev",
            "/explicit",
            "--rate",
            "44100",
            "--buffer-frames",
            "2048",
        ]
        .map(String::from);
        let result = complete(
            &args,
            SettingsHost::Linux,
            &NativeDefaults::for_validation(),
        )
        .unwrap();
        assert_eq!(
            result.chunks_exact(2).filter(|p| p[0] == "--rate").count(),
            1
        );
        assert!(result.chunks_exact(2).any(|p| p == ["--rate", "44100"]));
        assert!(
            result
                .chunks_exact(2)
                .any(|p| p == ["--evdev", "/explicit"])
        );
        assert!(result.chunks_exact(2).any(|p| p == ["--alsa", "default"]));
        assert_eq!(args.len(), 8);
        let solo = complete(
            &[],
            SettingsHost::Windows,
            &NativeDefaults::for_validation(),
        )
        .unwrap();
        assert!(!solo.iter().any(|s| s == "--keyboard-path"));
        assert!(solo.chunks_exact(2).any(|p| p == ["--mode", "shared"]));
    }
}
