//! Actual native entry-point parsing and cold validation. Foreign binaries only
//! exercise their parsers on this host; no physical input/audio claim is made.
#[cfg(target_os = "linux")]
use std::{fs, path::PathBuf};
use std::{
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

static SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug)]
enum Platform {
    Linux,
    Windows,
    Macos,
}
const PLATFORMS: [Platform; 3] = [Platform::Linux, Platform::Windows, Platform::Macos];

impl Platform {
    fn executable(self) -> &'static str {
        match self {
            Self::Linux => env!("CARGO_BIN_EXE_linux_bms"),
            Self::Windows => env!("CARGO_BIN_EXE_windows_bms"),
            Self::Macos => env!("CARGO_BIN_EXE_macos_bms"),
        }
    }

    fn is_native(self) -> bool {
        match self {
            Self::Linux => cfg!(target_os = "linux"),
            Self::Windows => cfg!(target_os = "windows"),
            Self::Macos => cfg!(target_os = "macos"),
        }
    }

    fn args(self, chart: &str, local_members: usize) -> Vec<String> {
        let mut args = vec!["--chart".into(), chart.into()];
        match self {
            Self::Linux => {
                append(
                    &mut args,
                    &[
                        "--alsa",
                        "null",
                        "--rate",
                        "48000",
                        "--channels",
                        "1",
                        "--period-frames",
                        "128",
                        "--buffer-frames",
                        "256",
                    ],
                );
                if local_members == 0 {
                    append(&mut args, &["--evdev", "/beatkernel-no-such-input/device"]);
                } else {
                    for player in 1..=local_members {
                        args.extend([
                            "--local-player".into(),
                            format!("{player}:/beatkernel-no-such-input/device-{player}"),
                        ]);
                    }
                }
            }
            Self::Windows => {
                append(
                    &mut args,
                    &["--device", "beatkernel-no-such-output", "--mode", "shared"],
                );
                for player in 1..=local_members {
                    args.extend([
                        "--local-player".into(),
                        format!("{player}:beatkernel-no-such-keyboard-{player}"),
                    ]);
                }
            }
            Self::Macos => {
                append(
                    &mut args,
                    &[
                        "--device",
                        "1",
                        "--rate",
                        "48000",
                        "--channels",
                        "1",
                        "--buffer-frames",
                        "256",
                    ],
                );
                if local_members == 0 {
                    append(&mut args, &["--keyboard-registry", "1"]);
                } else {
                    for player in 1..=local_members {
                        args.extend(["--local-player".into(), format!("{player}:{player}")]);
                    }
                }
            }
        }
        args
    }
}

fn append(args: &mut Vec<String>, extra: &[&str]) {
    args.extend(extra.iter().map(|s| (*s).to_owned()));
}

fn missing_chart() -> String {
    let path = std::env::temp_dir().join(format!(
        "beatkernel-live-cli-missing-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    assert!(!path.exists());
    path.join("chart.bms").display().to_string()
}

fn run(platform: Platform, args: &[String]) -> Output {
    let mut child = Command::new(platform.executable())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!(
                "{platform:?} CLI exceeded bounded cold validation: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn rejected(output: &Output) -> String {
    assert!(
        !output.status.success(),
        "unexpected success: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn assert_before_io(output: &Output) -> String {
    let error = rejected(output);
    assert!(
        output.stdout.is_empty(),
        "acquisition/preparation feedback before cold rejection: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    for forbidden in [
        "os error 2",
        "No such file",
        "requires Windows",
        "requires macOS",
        "requires Linux",
        "ALSA lib",
    ] {
        assert!(
            !error.contains(forbidden),
            "I/O or native platform acquisition preceded cold validation: {error}"
        );
    }
    error
}

#[test]
fn all_actual_help_describes_default_fraction_pitch_and_preserved_units() {
    for platform in PLATFORMS {
        let output = run(platform, &["--help".into()]);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        for required in [
            "--speed NUM/DEN",
            "1/1",
            "--seconds N",
            "--start-ns N",
            "--end-ns N",
            "--preroll-ns N",
        ] {
            assert!(
                help.contains(required),
                "{platform:?} missing {required}: {help}"
            );
        }
        assert!(help.to_ascii_lowercase().contains("pitch"), "{help}");
        match platform {
            Platform::Windows => assert!(help.contains("--buffer default|frames:N|ns:N")),
            _ => {
                assert!(help.contains("--rate HZ"));
                assert!(help.contains("--buffer-frames N"));
            }
        }
    }
}

#[test]
fn malformed_explicit_speed_is_not_empty_settings_default_and_rejects_before_io() {
    for platform in PLATFORMS {
        for speed in [
            "",
            "1",
            "/",
            "/1",
            "1/",
            "1/2/3",
            "0/1",
            "1/0",
            "-1/1",
            "1/-1",
            "+1/1",
            "1/+1",
            " 1/1",
            "1/1 ",
            "1.5/1",
            "１/1",
            "1/٢",
            "9223372036854775808/1",
            "1/18446744073709551616",
        ] {
            let mut args = platform.args(&missing_chart(), 0);
            append(&mut args, &["--speed", speed]);
            let error = assert_before_io(&run(platform, &args));
            assert!(
                error.to_ascii_lowercase().contains("speed")
                    || error.to_ascii_lowercase().contains("fraction")
                    || error.contains("number too large"),
                "{platform:?} invalid {speed:?} lacks fraction/type-overflow diagnostic: {error}"
            );
        }
    }
}

#[test]
fn duplicate_and_value_less_speed_reject_before_any_acquisition() {
    for platform in PLATFORMS {
        for extra in [
            vec!["--speed", "1/1", "--speed", "2/1"],
            vec!["--speed", "6/4", "--speed", "3/2"],
            vec!["--speed"],
        ] {
            let mut args = platform.args(&missing_chart(), 0);
            append(&mut args, &extra);
            let error = assert_before_io(&run(platform, &args));
            if extra.len() == 4 {
                assert!(error.to_ascii_lowercase().contains("duplicate"), "{error}");
            }
        }
    }
}

#[test]
fn representable_default_reduced_and_selected_speeds_reach_original_entry_path() {
    for platform in PLATFORMS {
        for members in [0, 2, 4] {
            for speed in [
                None,
                Some("1/1"),
                Some("1/2"),
                Some("2/1"),
                Some("3/2"),
                Some("6/4"),
                Some("0002/0002"),
                Some("9223372036854775807/18446744073709551615"),
            ] {
                let mut args = platform.args(&missing_chart(), members);
                if let Some(speed) = speed {
                    append(&mut args, &["--speed", speed]);
                }
                let error = rejected(&run(platform, &args));
                if platform.is_native() {
                    assert!(error.contains("os error 2") || error.contains("No such file"), "{platform:?} members={members} valid {speed:?} did not reach missing chart: {error}");
                } else {
                    assert!(
                        error.contains("requires Windows")
                            || error.contains("requires macOS")
                            || error.contains("requires Linux"),
                        "foreign parser did not reach explicit platform guard: {error}"
                    );
                }
            }
        }
    }
}

#[test]
fn maximum_positive_fraction_cold_preroll_underflow_is_rejected_for_solo_and_local() {
    for platform in PLATFORMS {
        for members in [0, 2, 4] {
            let mut args = platform.args(&missing_chart(), members);
            append(&mut args, &["--speed", "9223372036854775807/1"]);
            let error = assert_before_io(&run(platform, &args));
            let lower = error.to_ascii_lowercase();
            assert!(
                lower.contains("overflow")
                    || lower.contains("range")
                    || lower.contains("represent")
                    || lower.contains("transport"),
                "missing checked initial song time diagnostic: {error}"
            );
        }
    }
}

#[test]
fn finite_inverse_output_timestamp_overflow_rejects_before_chart_or_native_io() {
    for platform in PLATFORMS {
        let mut args = platform.args(&missing_chart(), 0);
        append(
            &mut args,
            &[
                "--speed",
                "1/18446744073709551615",
                "--preroll-ns",
                "0",
                "--start-ns",
                "0",
                "--end-ns",
                "1",
            ],
        );
        // A one-nanosecond original interval at this rate needs u64::MAX
        // output nanoseconds, outside the signed output timestamp domain.
        let error = assert_before_io(&run(platform, &args));
        let lower = error.to_ascii_lowercase();
        assert!(
            lower.contains("overflow") || lower.contains("range") || lower.contains("represent"),
            "finite inverse endpoint lacks checked output timestamp diagnostic: {error}"
        );
    }
}

#[test]
fn valid_original_finite_section_preroll_and_wall_cutoff_remain_accepted() {
    for platform in PLATFORMS {
        let mut args = platform.args(&missing_chart(), 0);
        append(
            &mut args,
            &[
                "--speed",
                "3/2",
                "--start-ns",
                "4000000",
                "--end-ns",
                "8000000",
                "--preroll-ns",
                "3000000",
                "--seconds",
                "1",
                "--voices",
                "16",
                "--input-offset-ns",
                "-1",
            ],
        );
        let error = rejected(&run(platform, &args));
        assert!(
            if platform.is_native() {
                error.contains("os error 2") || error.contains("No such file")
            } else {
                error.contains("requires Windows")
                    || error.contains("requires macOS")
                    || error.contains("requires Linux")
            },
            "preserved units unexpectedly rejected: {error}"
        );
        for extra in [
            vec!["--seconds", "0"],
            vec!["--preroll-ns", "10000000001"],
            vec!["--start-ns", "2", "--end-ns", "1"],
        ] {
            let mut invalid = platform.args(&missing_chart(), 0);
            append(&mut invalid, &["--speed", "3/2"]);
            append(&mut invalid, &extra);
            assert_before_io(&run(platform, &invalid));
        }
    }
}

#[test]
fn unimplemented_runtime_rate_and_retained_cli_controls_never_silently_fallback() {
    // These flags have no published consuming native CLI path. Their rejection
    // proves only cold CLI refusal, not interactive runtime control behavior.
    for platform in PLATFORMS {
        for flag in ["--seek-ns", "--restart", "--loop", "--runtime-speed"] {
            let mut args = platform.args(&missing_chart(), 0);
            append(&mut args, &["--speed", "3/2", flag, "1"]);
            let error = assert_before_io(&run(platform, &args));
            assert!(
                error.contains(flag),
                "unknown retained control lacks explicit diagnostic: {error}"
            );
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn actual_linux_feedback_normalizes_selected_speed_and_preserves_default_output() {
    struct Tree(PathBuf);
    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let tree = Tree(std::env::temp_dir().join(format!(
        "beatkernel-live-cli-chart-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&tree.0).unwrap();
    let chart = tree.0.join("chart.bms");
    fs::write(&chart, b"#BPM 60\n").unwrap();
    for (selected, normalized) in [
        (None, "1/1"),
        (Some("8/8"), "1/1"),
        (Some("6/4"), "3/2"),
        (Some("1/2"), "1/2"),
        (Some("2/1"), "2/1"),
    ] {
        let mut args = Platform::Linux.args(&chart.display().to_string(), 0);
        if let Some(selected) = selected {
            append(&mut args, &["--speed", selected]);
        }
        let output = run(Platform::Linux, &args);
        let error = rejected(&output);
        assert!(
            error.contains("os error 2") || error.contains("No such file"),
            "expected existing collector refusal for absent physical device: {error}"
        );
        let feedback = String::from_utf8(output.stdout).unwrap();
        if normalized == "1/1" {
            // The documented NORMAL compatibility branch retains the existing
            // entry point rather than adding selected-speed product feedback.
            assert!(
                !feedback.contains("live playback speed="),
                "default setup acquired nonnormal feedback: {feedback}"
            );
            assert!(
                feedback.contains("prepared practice section="),
                "actual original chart was not prepared: {feedback}"
            );
        } else {
            assert!(
                feedback.contains(&format!("speed={normalized}")),
                "normalized setup feedback absent: {feedback}"
            );
            assert!(
                feedback.to_ascii_lowercase().contains("pitch"),
                "pitch-changing feedback absent: {feedback}"
            );
        }
    }
}
