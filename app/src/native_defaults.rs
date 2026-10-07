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
/// Application preparation owns policy; adapters return actual native metadata.
pub type NativePreparationResult<T> = Result<T, Box<dyn std::error::Error>>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreparationMode {
    Play,
    Replay,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeOutputFormat {
    pub rate: f64,
    pub channels: u32,
    pub buffer_frames: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeKeyboardCandidate {
    pub id: String,
    pub selectable: bool,
    pub order: u64,
}
pub trait NativePreparationDevice {
    fn default_output(&mut self) -> NativePreparationResult<String>;
    fn output_format(&mut self, device: &str) -> NativePreparationResult<NativeOutputFormat>;
    fn keyboards(&mut self) -> NativePreparationResult<Vec<NativeKeyboardCandidate>>;
    fn asio_format(&mut self, projected: &[String]) -> NativePreparationResult<NativeOutputFormat>;
}
fn metadata_id(id: &str) -> NativePreparationResult<()> {
    if id.is_empty()
        || id.len() > crate::device_catalog::MAX_DEVICE_TEXT_BYTES
        || id
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return Err("native metadata has an invalid bounded identity".into());
    }
    Ok(())
}
fn native_format(
    format: NativeOutputFormat,
    host: SettingsHost,
) -> NativePreparationResult<(u32, u16, Option<u32>)> {
    if !format.rate.is_finite()
        || format.rate <= 0.0
        || format.rate.fract() != 0.0
        || format.rate > f64::from(u32::MAX)
        || format.channels == 0
        || format.buffer_frames == Some(0)
    {
        return Err("native output metadata has invalid rate/channels/buffer".into());
    }
    let channels = if host == SettingsHost::Macos {
        format.channels.min(2)
    } else {
        format.channels
    };
    let channels = u16::try_from(channels)?;
    beatkernel::audio::AudioFormat::new(format.rate as u32, channels)?;
    Ok((format.rate as u32, channels, format.buffer_frames))
}
/// Resolve omitted application values only, after strict original-argument validation.
/// No parser-only device/keyboard identity is ever used for native execution.
pub fn prepare<D: NativePreparationDevice>(
    args: &[String],
    host: SettingsHost,
    mode: PreparationMode,
    device: &mut D,
    validate: impl FnOnce(&[String]) -> NativePreparationResult<()>,
) -> NativePreparationResult<Vec<String>> {
    validate(args)?;
    // Keep schema/size rejection ahead of native calls even for a permissive
    // supplied validator. Existing projection remains the schema authority.
    let mut defaults = NativeDefaults {
        device: String::new(),
        keyboard: String::new(),
        rate: 48_000,
        channels: 2,
        buffer_frames: 1024,
    };
    let mut validation_defaults = NativeDefaults::for_validation();
    if host == SettingsHost::Macos {
        validation_defaults.device = "1".into();
    }
    match mode {
        PreparationMode::Play => {
            complete(args, host, &validation_defaults)?;
        }
        PreparationMode::Replay => {
            replay_args(args, host, &validation_defaults)?;
        }
    }
    let get = |flag: &str| {
        args.chunks_exact(2)
            .find(|pair| pair[0] == flag)
            .map(|pair| pair[1].as_str())
    };
    let asio = host == SettingsHost::Windows && get("--backend") == Some("asio");
    if asio && get("--device").is_none() {
        return Err("ASIO requires an explicit driver configuration".into());
    }
    if host != SettingsHost::Linux {
        if let Some(id) = get("--device") {
            defaults.device = id.to_owned();
        } else {
            defaults.device = device.default_output()?;
            metadata_id(&defaults.device)?;
        }
    }
    let need_format = match host {
        SettingsHost::Linux => false,
        SettingsHost::Windows => {
            mode == PreparationMode::Replay
                && !asio
                && (get("--rate").is_none() || get("--channels").is_none())
        }
        SettingsHost::Macos => {
            get("--rate").is_none()
                || get("--channels").is_none()
                || get("--buffer-frames").is_none()
        }
    };
    if need_format {
        let format = device.output_format(&defaults.device)?;
        let (rate, channels, buffer) = native_format(format, host)?;
        defaults.rate = rate;
        defaults.channels = channels;
        if host == SettingsHost::Macos && get("--buffer-frames").is_none() {
            defaults.buffer_frames = buffer.ok_or("native output buffer metadata missing")?;
        }
    }
    if asio && mode == PreparationMode::Replay && get("--rate").is_none() {
        let projected = replay_args(args, host, &defaults)?;
        let format = device.asio_format(&projected)?;
        let (rate, _, _) = native_format(format, host)?;
        defaults.rate = rate;
    }
    let need_keyboard = mode == PreparationMode::Play
        && get("--local-player").is_none()
        && match host {
            SettingsHost::Linux => get("--evdev").is_none() && get("--local-input").is_none(),
            SettingsHost::Macos => get("--keyboard-registry").is_none(),
            SettingsHost::Windows => false,
        };
    if need_keyboard {
        let candidates = device.keyboards()?;
        if candidates.len() > crate::device_catalog::MAX_DEVICES {
            return Err("native keyboard metadata exceeds device count".into());
        }
        let mut bytes = 0usize;
        let mut selected: Option<&NativeKeyboardCandidate> = None;
        for candidate in &candidates {
            metadata_id(&candidate.id)?;
            bytes = bytes
                .checked_add(candidate.id.len())
                .ok_or("native keyboard metadata byte overflow")?;
            if bytes > crate::device_catalog::MAX_CATALOG_BYTES {
                return Err("native keyboard metadata exceeds byte budget".into());
            }
            if candidate.selectable
                && selected.is_none_or(|previous| candidate.order < previous.order)
            {
                selected = Some(candidate);
            }
        }
        defaults.keyboard = selected
            .ok_or("no selectable native keyboard available")?
            .id
            .clone();
    }
    // Unused missing identities remain empty: complete never inserts them for
    // explicit inputs/assigned players, replay never inserts input options.
    Ok(match mode {
        PreparationMode::Play => complete(args, host, &defaults)?,
        PreparationMode::Replay => replay_args(args, host, &defaults)?,
    })
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

#[cfg(test)]
mod chart_seed_projection_fixtures {
    use super::*;
    #[test]
    fn live_defaults_keep_seed_but_watch_uses_recorded_provenance() {
        for host in [
            SettingsHost::Windows,
            SettingsHost::Linux,
            SettingsHost::Macos,
        ] {
            let args = [
                "--chart",
                "random.bms",
                "--chart-seed",
                "18446744073709551615",
            ]
            .map(String::from);
            let defaults = NativeDefaults::for_validation();
            let live = complete(&args, host, &defaults).unwrap();
            assert_eq!(
                NativeSettings::from_args(&live, host).unwrap().chart_seed(),
                Ok(u64::MAX)
            );
            let watch = args
                .into_iter()
                .chain(["--replay".into(), "record.bkr".into()])
                .collect::<Vec<_>>();
            let projected = replay_args(&watch, host, &defaults).unwrap();
            assert!(!projected.iter().any(|arg| arg == "--chart-seed"));
            assert!(
                projected
                    .chunks_exact(2)
                    .any(|pair| pair == ["--replay", "record.bkr"])
            );
        }
    }
}

#[cfg(test)]
mod preparation_fixtures {
    use super::*;
    #[derive(Default)]
    struct Device {
        calls: Vec<String>,
        projected: Vec<String>,
        bad: Option<NativeOutputFormat>,
        bad_id: bool,
        keyboard_count: usize,
        failure: bool,
    }
    impl NativePreparationDevice for Device {
        fn default_output(&mut self) -> NativePreparationResult<String> {
            self.calls.push("default".into());
            if self.failure {
                return Err("default unavailable".into());
            }
            Ok(if self.bad_id {
                "bad\nID".into()
            } else {
                "7".into()
            })
        }
        fn output_format(&mut self, id: &str) -> NativePreparationResult<NativeOutputFormat> {
            self.calls.push(format!("format:{id}"));
            if self.failure {
                return Err("format unavailable".into());
            }
            Ok(self.bad.unwrap_or(NativeOutputFormat {
                rate: 44_100.0,
                channels: 8,
                buffer_frames: Some(512),
            }))
        }
        fn keyboards(&mut self) -> NativePreparationResult<Vec<NativeKeyboardCandidate>> {
            self.calls.push("keyboard".into());
            if self.keyboard_count > 0 {
                return Ok((0..self.keyboard_count)
                    .map(|order| NativeKeyboardCandidate {
                        id: "1".into(),
                        selectable: true,
                        order: order as u64,
                    })
                    .collect());
            }
            Ok(vec![
                NativeKeyboardCandidate {
                    id: "disabled".into(),
                    selectable: false,
                    order: 0,
                },
                NativeKeyboardCandidate {
                    id: "9".into(),
                    selectable: true,
                    order: 9,
                },
                NativeKeyboardCandidate {
                    id: "3".into(),
                    selectable: true,
                    order: 3,
                },
            ])
        }
        fn asio_format(&mut self, args: &[String]) -> NativePreparationResult<NativeOutputFormat> {
            self.calls.push("asio".into());
            self.projected = args.to_vec();
            Ok(NativeOutputFormat {
                rate: 96_000.0,
                channels: 2,
                buffer_frames: None,
            })
        }
    }
    fn args(replay: bool) -> Vec<String> {
        let mut args = vec!["--chart".into(), "song.bms".into()];
        if replay {
            args.extend(["--replay".into(), "past.bkr".into()]);
        }
        args
    }
    fn validated(
        args: &[String],
        host: SettingsHost,
        mode: PreparationMode,
    ) -> NativePreparationResult<()> {
        let mut defaults = NativeDefaults::for_validation();
        if host == SettingsHost::Macos {
            defaults.device = "1".into();
        }
        match mode {
            PreparationMode::Play => {
                complete(args, host, &defaults)?;
            }
            PreparationMode::Replay => {
                replay_args(args, host, &defaults)?;
            }
        }
        Ok(())
    }
    fn run(
        args: &[String],
        host: SettingsHost,
        mode: PreparationMode,
        device: &mut Device,
    ) -> NativePreparationResult<Vec<String>> {
        prepare(args, host, mode, device, |args| validated(args, host, mode))
    }
    #[test]
    fn shared_preparation_uses_minimum_actual_host_mode_queries() {
        for (host, mode, expected) in [
            (
                SettingsHost::Windows,
                PreparationMode::Play,
                vec!["default"],
            ),
            (
                SettingsHost::Windows,
                PreparationMode::Replay,
                vec!["default", "format:7"],
            ),
            (SettingsHost::Linux, PreparationMode::Play, vec!["keyboard"]),
            (SettingsHost::Linux, PreparationMode::Replay, vec![]),
            (
                SettingsHost::Macos,
                PreparationMode::Play,
                vec!["default", "format:7", "keyboard"],
            ),
            (
                SettingsHost::Macos,
                PreparationMode::Replay,
                vec!["default", "format:7"],
            ),
        ] {
            let original = args(mode == PreparationMode::Replay);
            let mut device = Device::default();
            let output = run(&original, host, mode, &mut device).unwrap();
            assert_eq!(device.calls, expected);
            assert_eq!(original, args(mode == PreparationMode::Replay));
            if host == SettingsHost::Macos {
                assert!(
                    output
                        .chunks_exact(2)
                        .any(|pair| pair == ["--channels", "2"])
                );
                assert!(
                    output
                        .chunks_exact(2)
                        .any(|pair| pair == ["--buffer-frames", "512"])
                );
            }
            if mode == PreparationMode::Replay {
                assert!(!output.iter().any(|arg| matches!(
                    arg.as_str(),
                    "--evdev" | "--keyboard-path" | "--keyboard-registry"
                )));
            }
            if host == SettingsHost::Linux && mode == PreparationMode::Play {
                assert!(output.chunks_exact(2).any(|pair| pair == ["--evdev", "3"]));
            }
        }
    }
    #[test]
    fn explicit_options_and_group_assignments_never_trigger_unnecessary_queries() {
        for host in [
            SettingsHost::Windows,
            SettingsHost::Linux,
            SettingsHost::Macos,
        ] {
            let mut values = args(false);
            if host == SettingsHost::Windows {
                values.extend(["--buffer", "frames:64"].map(String::from));
            } else {
                values.extend(
                    [
                        "--rate",
                        "32000",
                        "--channels",
                        "1",
                        "--buffer-frames",
                        "64",
                    ]
                    .map(String::from),
                );
            }
            values.extend(match host {
                SettingsHost::Windows => vec!["--device".into(), "explicit".into()],
                SettingsHost::Linux => vec![
                    "--alsa".into(),
                    "explicit".into(),
                    "--evdev".into(),
                    "/explicit".into(),
                ],
                SettingsHost::Macos => vec![
                    "--device".into(),
                    "99".into(),
                    "--keyboard-registry".into(),
                    "88".into(),
                ],
            });
            let mut device = Device::default();
            let output = run(&values, host, PreparationMode::Play, &mut device).unwrap();
            assert!(device.calls.is_empty());
            assert_eq!(&output[..values.len()], &values);
        }
        for count in [2, 3, 4] {
            for host in [SettingsHost::Linux, SettingsHost::Macos] {
                let mut values = args(false);
                for id in 1..=count {
                    let input = if host == SettingsHost::Linux {
                        format!("{id}:/dev/input/event{id}")
                    } else {
                        format!("{id}:{}", 100 + id)
                    };
                    values.extend(["--local-player".into(), input]);
                }
                let mut device = Device::default();
                run(&values, host, PreparationMode::Play, &mut device).unwrap();
                assert!(!device.calls.iter().any(|call| call == "keyboard"));
            }
        }
    }
    #[test]
    fn original_validator_and_schema_failure_precede_all_native_calls() {
        let values = args(false);
        let mut device = Device::default();
        let original = values.clone();
        assert!(
            prepare(
                &values,
                SettingsHost::Macos,
                PreparationMode::Play,
                &mut device,
                |received| {
                    assert_eq!(received, original);
                    Err("strict syntax rejected".into())
                }
            )
            .is_err()
        );
        assert!(device.calls.is_empty());
        let invalid = ["--unknown".into(), "bad".into()];
        assert!(
            prepare(
                &invalid,
                SettingsHost::Windows,
                PreparationMode::Play,
                &mut device,
                |_| Ok(())
            )
            .is_err()
        );
        assert!(device.calls.is_empty());
        let mut asio = args(false);
        asio.extend(["--backend".into(), "asio".into()]);
        assert!(
            run(
                &asio,
                SettingsHost::Windows,
                PreparationMode::Play,
                &mut device
            )
            .is_err()
        );
        assert!(device.calls.is_empty());
    }
    #[test]
    fn metadata_errors_never_become_parser_placeholder_execution() {
        for format in [
            NativeOutputFormat {
                rate: f64::NAN,
                channels: 2,
                buffer_frames: Some(512),
            },
            NativeOutputFormat {
                rate: 44100.5,
                channels: 2,
                buffer_frames: Some(512),
            },
            NativeOutputFormat {
                rate: 0.0,
                channels: 2,
                buffer_frames: Some(512),
            },
            NativeOutputFormat {
                rate: f64::from(u32::MAX) + 1.0,
                channels: 2,
                buffer_frames: Some(512),
            },
            NativeOutputFormat {
                rate: 48000.0,
                channels: 0,
                buffer_frames: Some(512),
            },
            NativeOutputFormat {
                rate: 48000.0,
                channels: 2,
                buffer_frames: Some(0),
            },
            NativeOutputFormat {
                rate: 48000.0,
                channels: 2,
                buffer_frames: None,
            },
        ] {
            let mut device = Device {
                bad: Some(format),
                ..Default::default()
            };
            assert!(
                run(
                    &args(false),
                    SettingsHost::Macos,
                    PreparationMode::Play,
                    &mut device
                )
                .is_err()
            );
            assert!(!device.calls.iter().any(|call| call == "keyboard"));
        }
        for (bad_id, failure) in [(true, false), (false, true)] {
            let mut device = Device {
                bad_id,
                failure,
                ..Default::default()
            };
            assert!(
                run(
                    &args(false),
                    SettingsHost::Windows,
                    PreparationMode::Play,
                    &mut device
                )
                .is_err()
            );
            assert_eq!(device.calls, vec!["default"]);
        }
        let mut device = Device {
            keyboard_count: crate::device_catalog::MAX_DEVICES + 1,
            ..Default::default()
        };
        assert!(
            run(
                &args(false),
                SettingsHost::Linux,
                PreparationMode::Play,
                &mut device
            )
            .is_err()
        );
        assert!(metadata_id("").is_err());
        assert!(
            metadata_id(&"x".repeat(crate::device_catalog::MAX_DEVICE_TEXT_BYTES + 1)).is_err()
        );
    }
    #[test]
    fn asio_replay_query_uses_real_projection_routing_buffer_and_clock_assessments() {
        let mut values = args(true);
        values.extend(
            [
                "--backend",
                "asio",
                "--device",
                "{12345678-9ABC-DEF0-1234-56789ABCDEF0}",
                "--output-channels",
                "3,7",
                "--buffer",
                "frames:64",
                "--asio-system-clock",
                "multimedia",
                "--asio-timer-error-ns",
                "100",
            ]
            .map(String::from),
        );
        let mut device = Device::default();
        let output = run(
            &values,
            SettingsHost::Windows,
            PreparationMode::Replay,
            &mut device,
        )
        .unwrap();
        assert_eq!(device.calls, vec!["asio"]);
        for pair in [
            ["--channels", "2"],
            ["--output-channels", "3,7"],
            ["--buffer-frames", "64"],
            ["--asio-system-clock", "multimedia"],
            ["--asio-timer-error-ns", "100"],
        ] {
            assert!(device.projected.chunks_exact(2).any(|p| p == pair));
        }
        assert!(output.chunks_exact(2).any(|p| p == ["--rate", "96000"]));
        assert!(!device.projected.iter().any(|arg| arg == "--bind"));
        let live: Vec<String> = values
            .chunks_exact(2)
            .filter(|pair| pair[0] != "--replay")
            .flatten()
            .cloned()
            .collect();
        let mut device = Device::default();
        run(
            &live,
            SettingsHost::Windows,
            PreparationMode::Play,
            &mut device,
        )
        .unwrap();
        assert!(device.calls.is_empty());
    }
}
