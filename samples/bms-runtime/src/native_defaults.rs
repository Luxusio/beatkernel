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
            if get("--local-input").is_none() && get("--local-player").is_none() {
                add("--evdev", defaults.keyboard.clone());
            }
            add("--rate", defaults.rate.to_string());
            add("--channels", defaults.channels.to_string());
            add("--period-frames", "256".into());
            add("--buffer-frames", defaults.buffer_frames.to_string());
        }
        SettingsHost::Macos => {
            add("--device", defaults.device.clone());
            if get("--local-player").is_none() {
                add("--keyboard-registry", defaults.keyboard.clone());
            }
            add("--rate", defaults.rate.to_string());
            add("--channels", defaults.channels.to_string());
            add("--buffer-frames", defaults.buffer_frames.to_string());
        }
    }
    add("--channel-policy", "mono-stereo".into());
    NativeSettings::from_args(&result, host)?;
    Ok(result)
}
/// Project a graphical draft into recorded output options without live acquisition.
/// The recording owns judging/profile/start; omitted device metadata is supplied
/// by the game owner. This function never discovers devices or opens files.
pub fn replay_args(
    args: &[String],
    host: SettingsHost,
    defaults: &NativeDefaults,
) -> Result<Vec<String>, String> {
    use crate::settings::{MAX_FIELDS, MAX_TOTAL_BYTES, MAX_VALUE_BYTES};
    if args.len() % 2 != 0 || args.len() / 2 > MAX_FIELDS + 1 {
        return Err("replay invocation exceeds bounded flag/value pairs".into());
    }
    let mut native = Vec::new();
    let mut replay = None;
    let mut chart = None;
    let mut total = 0usize;
    for pair in args.chunks_exact(2) {
        if pair[1].is_empty()
            || pair[1].len() > MAX_VALUE_BYTES
            || pair[1]
                .chars()
                .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
        {
            return Err("invalid bounded replay option value".into());
        }
        total = total
            .checked_add(pair[1].len())
            .ok_or("replay option byte overflow")?;
        match pair[0].as_str() {
            "--replay" if replay.is_none() => replay = Some(pair[1].clone()),
            "--replay" => return Err("duplicate replay path".into()),
            "--chart" => {
                if chart.replace(pair[1].clone()).is_some() {
                    return Err("duplicate chart path".into());
                }
                native.extend(pair.iter().cloned());
            }
            _ => native.extend(pair.iter().cloned()),
        }
    }
    if total > MAX_TOTAL_BYTES + MAX_VALUE_BYTES {
        return Err("replay options exceed byte limit".into());
    }
    let replay = replay.ok_or("replay path required")?;
    chart.ok_or("chart path required")?;
    NativeSettings::from_args(&native, host)?;
    let asio = native.chunks_exact(2).any(|p| p == ["--backend", "asio"]);
    let routed_channels = native
        .chunks_exact(2)
        .find(|p| p[0] == "--output-channels")
        .map(|p| p[1].split(',').count());
    let mut result = Vec::new();
    for pair in native.chunks_exact(2) {
        if asio && pair[0] == "--buffer" {
            if pair[1] == "default" {
                continue;
            }
            let frames = pair[1]
                .strip_prefix("frames:")
                .ok_or("ASIO replay buffer must be driver default or exact frames")?;
            result.extend(["--buffer-frames".into(), frames.to_owned()]);
            continue;
        }
        let flag = match pair[0].as_str() {
            "--chart" | "--device" | "--backend" | "--mode" | "--shared-policy" | "--asio-view"
            | "--output-channels" | "--rate" | "--channels" | "--buffer-frames"
            | "--period-frames" | "--preroll-ns" | "--voices" | "--seconds"
            | "--channel-policy" => pair[0].as_str(),
            "--asio-system-clock"
            | "--asio-timer-error-ns"
            | "--asio-drift-error-ns"
            | "--asio-latency-error-ns"
            | "--asio-anchor-age-ns" => pair[0].as_str(),
            "--alsa" => "--device",
            "--bgm-lookahead-ns" => "--lookahead-ns",
            "--replay-max-records" => "--max-records",
            "--replay-max-bytes" => "--max-bytes",
            _ => continue,
        };
        result.extend([flag.to_owned(), pair[1].clone()]);
    }
    let mut add = |flag: &str, value: String| {
        if !result.chunks_exact(2).any(|p| p[0] == flag) {
            result.extend([flag.to_owned(), value]);
        }
    };
    add(
        "--device",
        if host == SettingsHost::Linux {
            "default".into()
        } else {
            defaults.device.clone()
        },
    );
    if host == SettingsHost::Windows && !asio {
        add("--mode", "shared".into());
    }
    add("--rate", defaults.rate.to_string());
    add(
        "--channels",
        if asio {
            routed_channels
                .ok_or("ASIO replay output routing required")?
                .to_string()
        } else {
            defaults.channels.to_string()
        },
    );
    if host != SettingsHost::Windows {
        add("--buffer-frames", defaults.buffer_frames.to_string());
    }
    if host == SettingsHost::Linux {
        add("--period-frames", "256".into());
    }
    add("--channel-policy", "mono-stereo".into());
    result.extend(["--replay".into(), replay]);
    Ok(result)
}
#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn asio_watch_preserves_clock_assessments_exact_buffer_and_routed_channel_count() {
        let args = [
            "--chart",
            "song.bms",
            "--replay",
            "past.bkr",
            "--backend",
            "asio",
            "--device",
            "{12345678-9ABC-DEF0-1234-56789ABCDEF0}",
            "--asio-view",
            "native",
            "--output-channels",
            "3",
            "--buffer",
            "frames:64",
            "--asio-system-clock",
            "multimedia",
            "--asio-timer-error-ns",
            "100",
            "--asio-drift-error-ns",
            "200",
            "--asio-latency-error-ns",
            "300",
            "--asio-anchor-age-ns",
            "500000000",
        ]
        .map(String::from);
        let out = replay_args(
            &args,
            SettingsHost::Windows,
            &NativeDefaults::for_validation(),
        )
        .unwrap();
        for pair in [
            ["--buffer-frames", "64"],
            ["--channels", "1"],
            ["--output-channels", "3"],
            ["--asio-system-clock", "multimedia"],
            ["--asio-timer-error-ns", "100"],
            ["--asio-drift-error-ns", "200"],
            ["--asio-latency-error-ns", "300"],
            ["--asio-anchor-age-ns", "500000000"],
        ] {
            assert!(out.chunks_exact(2).any(|p| p == pair));
        }
        assert!(!out.iter().any(|s| s == "--mode" || s == "--buffer"));
        let mut preferred = args.to_vec();
        let index = preferred.iter().position(|s| s == "--buffer").unwrap();
        preferred[index + 1] = "default".into();
        let out = replay_args(
            &preferred,
            SettingsHost::Windows,
            &NativeDefaults::for_validation(),
        )
        .unwrap();
        assert!(!out.iter().any(|s| s == "--buffer-frames"));
    }
    #[test]
    fn recorded_output_projection_omits_live_state_and_preserves_exact_output() {
        let args = [
            "--chart",
            "song.bms",
            "--replay",
            "past.bkr",
            "--local-player",
            "7:/a",
            "--local-player",
            "99:/b",
            "--record-replay",
            "new.bkr",
            "--ghost-other",
            "ghost.bkr",
            "--mp-host",
            "127.0.0.1:1234",
            "--start-ns",
            "123",
            "--input-offset-ns",
            "9",
            "--alsa",
            "hw:2",
            "--rate",
            "44100",
            "--buffer-frames",
            "2048",
            "--bgm-lookahead-ns",
            "9000000",
            "--replay-max-records",
            "100",
        ]
        .map(String::from);
        let original = args.clone();
        let out = replay_args(
            &args,
            SettingsHost::Linux,
            &NativeDefaults::for_validation(),
        )
        .unwrap();
        for flag in [
            "--local-player",
            "--record-replay",
            "--ghost-other",
            "--mp-host",
            "--start-ns",
            "--input-offset-ns",
        ] {
            assert!(!out.iter().any(|s| s == flag));
        }
        for pair in [
            ["--device", "hw:2"],
            ["--rate", "44100"],
            ["--buffer-frames", "2048"],
            ["--lookahead-ns", "9000000"],
            ["--max-records", "100"],
            ["--replay", "past.bkr"],
        ] {
            assert!(out.chunks_exact(2).any(|p| p == pair));
        }
        assert_eq!(args, original);
    }
    #[test]
    fn replay_defaults_are_output_only_on_every_host_and_paths_are_bounded() {
        let args = ["--chart", "song.bms", "--replay", "r.bkr"].map(String::from);
        for host in [
            SettingsHost::Windows,
            SettingsHost::Linux,
            SettingsHost::Macos,
        ] {
            let out = replay_args(&args, host, &NativeDefaults::for_validation()).unwrap();
            assert!(out.chunks_exact(2).any(|p| p == ["--rate", "48000"]));
            assert!(out.chunks_exact(2).any(|p| p == ["--channels", "2"]));
            assert!(!out.iter().any(|s| matches!(
                s.as_str(),
                "--evdev" | "--keyboard-path" | "--keyboard-registry"
            )));
        }
        let defaults = NativeDefaults::for_validation();
        assert!(replay_args(&args[..2], SettingsHost::Linux, &defaults).is_err());
        let mut duplicate = args.to_vec();
        duplicate.extend(["--replay".into(), "x".into()]);
        assert!(replay_args(&duplicate, SettingsHost::Linux, &defaults).is_err());
        let mut bad = args.to_vec();
        bad[3] = "x".repeat(crate::settings::MAX_VALUE_BYTES + 1);
        assert!(replay_args(&bad, SettingsHost::Linux, &defaults).is_err());
        bad[3] = "bad\npath".into();
        assert!(replay_args(&bad, SettingsHost::Linux, &defaults).is_err());
    }
    #[test]
    fn stable_local_devices_do_not_add_a_solo_input_override() {
        let args = [
            "--local-player",
            "7:/dev/input/event1",
            "--local-player",
            "1000:/dev/input/event2",
        ]
        .map(String::from);
        let completed = complete(
            &args,
            SettingsHost::Linux,
            &NativeDefaults::for_validation(),
        )
        .unwrap();
        assert!(!completed.iter().any(|s| s == "--evdev"));
        assert_eq!(
            completed
                .chunks_exact(2)
                .filter(|p| p[0] == "--local-player")
                .count(),
            2
        );
    }
    #[test]
    fn local_devices_suppress_solo_default_and_preserve_all_assignments() {
        let args = [
            "--local-input",
            "/dev/input/event1",
            "--local-input",
            "/dev/input/event2",
            "--local-input",
            "/dev/input/event3",
            "--local-input",
            "/dev/input/event4",
        ]
        .map(String::from);
        let completed = complete(
            &args,
            SettingsHost::Linux,
            &NativeDefaults::for_validation(),
        )
        .unwrap();
        assert!(!completed.iter().any(|s| s == "--evdev"));
        let devices: Vec<_> = completed
            .chunks_exact(2)
            .filter(|p| p[0] == "--local-input")
            .map(|p| p[1].as_str())
            .collect();
        assert_eq!(
            devices,
            [
                "/dev/input/event1",
                "/dev/input/event2",
                "/dev/input/event3",
                "/dev/input/event4"
            ]
        );
        assert!(
            complete(
                &args,
                SettingsHost::Windows,
                &NativeDefaults::for_validation()
            )
            .is_err()
        );
        assert!(
            complete(
                &args,
                SettingsHost::Macos,
                &NativeDefaults::for_validation()
            )
            .is_err()
        );
    }
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
    #[test]
    fn macos_group_defaults_never_inject_a_solo_registry() {
        let group = vec![
            "--local-player".into(),
            "1:100".into(),
            "--local-player".into(),
            "9:200".into(),
        ];
        let complete = super::complete(
            &group,
            SettingsHost::Macos,
            &NativeDefaults::for_validation(),
        )
        .unwrap();
        assert!(!complete.iter().any(|arg| arg == "--keyboard-registry"));
        assert_eq!(
            complete
                .iter()
                .filter(|arg| arg.as_str() == "--local-player")
                .count(),
            2
        );
    }
}
