//! Actual CLI validation before original file/device acquisition. No hardware claim.
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static SERIAL: AtomicU64 = AtomicU64::new(0);

fn run(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_play_replay_bms"))
        .args(args)
        .output()
        .unwrap()
}

fn args() -> Vec<String> {
    let missing = std::env::temp_dir().join(format!(
        "beatkernel-native-speed-missing-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    assert!(!missing.exists());
    let mut args = vec![
        "--chart".into(),
        missing.join("chart.bms").display().to_string(),
        "--replay".into(),
        missing.join("record.bkr").display().to_string(),
        "--device".into(),
        if cfg!(target_os = "macos") {
            "1"
        } else {
            "beatkernel-nonexistent-device"
        }
        .into(),
        "--rate".into(),
        "48000".into(),
        "--channels".into(),
        "1".into(),
    ];
    if cfg!(target_os = "linux") {
        args.extend(["--buffer-frames", "256", "--period-frames", "128"].map(str::to_owned));
    } else if cfg!(target_os = "macos") {
        args.extend(["--buffer-frames", "256"].map(str::to_owned));
    }
    args
}

fn failure(out: Output) -> String {
    assert!(!out.status.success(), "unexpected success");
    assert!(
        out.stdout.is_empty(),
        "success metadata on rejection: {:?}",
        out.stdout
    );
    String::from_utf8(out.stderr).unwrap()
}

#[test]
fn actual_help_distinguishes_pitch_changing_speed_device_hz_and_wall_cutoff() {
    let out = run(&["--help".into()]);
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let help = String::from_utf8(out.stdout).unwrap();
    for text in [
        "--speed NUM/DEN",
        "1/1",
        "--rate HZ",
        "--output-rate HZ",
        "--seconds N",
    ] {
        assert!(help.contains(text), "missing {text}: {help}");
    }
    assert!(help.to_ascii_lowercase().contains("pitch"));
}

#[test]
fn malformed_speed_rejects_before_missing_files_or_device() {
    for value in [
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
        "999999999999999999999999999/1",
    ] {
        let mut args = args();
        args.extend(["--speed".to_owned(), value.to_owned()]);
        let err = failure(run(&args));
        assert!(
            !err.contains("os error 2") && !err.contains("No such file"),
            "file access before invalid {value:?}: {err}"
        );
        assert!(
            !err.to_ascii_lowercase().contains("alsa"),
            "device acquisition before validation: {err}"
        );
    }
}

#[test]
fn duplicate_and_missing_speed_are_syntax_errors_before_io() {
    for suffix in [
        vec!["--speed", "1/1", "--speed", "2/1"],
        vec!["--speed", "1/1", "--speed", "1/1"],
        vec!["--speed"],
    ] {
        let mut args = args();
        args.extend(suffix.iter().map(|s| (*s).to_owned()));
        let err = failure(run(&args));
        assert!(
            !err.contains("os error 2") && !err.contains("No such file"),
            "{err}"
        );
        if suffix.len() == 4 {
            assert!(err.to_ascii_lowercase().contains("duplicate"), "{err}");
        }
    }
}

#[test]
fn accepted_positive_ratios_reach_original_replay_file_before_device() {
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
        let mut args = args();
        if let Some(speed) = speed {
            args.extend(["--speed".to_owned(), speed.to_owned()]);
        }
        let err = failure(run(&args));
        assert!(
            err.contains("os error 2") || err.contains("No such file"),
            "accepted {speed:?} did not reach missing original replay: {err}"
        );
    }
}
