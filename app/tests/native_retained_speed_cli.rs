//! Shipped retained-playback invocation contracts. Linux uses a real chart/WAV,
//! ALSA's null endpoint and deliberately absent evdev paths: reaching collector
//! initialization proves cold preparation, never native playback or presentation.
//! Foreign entry points exercise parsing and their explicit platform guards only.
use std::{
    io::Read,
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};
#[cfg(target_os = "linux")]
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const OUTPUT_LIMIT: usize = 1024 * 1024;
const STANDALONE: [(&str, &str); 3] = [
    ("linux", env!("CARGO_BIN_EXE_linux_bms")),
    ("windows", env!("CARGO_BIN_EXE_windows_bms")),
    ("macos", env!("CARGO_BIN_EXE_macos_bms")),
];
const PRIMARY: &str = env!("CARGO_BIN_EXE_beatkernel-bms-runtime");

/// Kill and reap on timeout or an assertion/unwind while handling the process.
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// Drain both pipes concurrently so even unexpected diagnostics cannot block the
// child before the watchdog runs. Bound retained bytes, while still draining.
fn drain(mut pipe: impl Read) -> (Vec<u8>, bool) {
    let mut kept = Vec::new();
    let mut truncated = false;
    let mut chunk = [0u8; 4096];
    loop {
        let count = match pipe.read(&mut chunk) {
            Ok(0) => return (kept, truncated),
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("CLI output read failed: {error}"),
        };
        let retain = count.min(OUTPUT_LIMIT - kept.len());
        kept.extend_from_slice(&chunk[..retain]);
        truncated |= retain < count;
    }
}

fn invoke(executable: &str, args: &[String]) -> Output {
    let mut process = Process(
        Command::new(executable)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = process.0.stdout.take().unwrap();
    let stderr = process.0.stderr.take().unwrap();
    let stdout = thread::spawn(move || drain(stdout));
    let stderr = thread::spawn(move || drain(stderr));
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = process.0.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            process.0.kill().unwrap();
            break process.0.wait().unwrap();
        }
        thread::sleep(Duration::from_millis(5));
    };
    let (stdout, stdout_truncated) = stdout.join().unwrap();
    let (stderr, stderr_truncated) = stderr.join().unwrap();
    assert!(
        !timed_out,
        "{executable} exceeded cold CLI deadline: {}",
        String::from_utf8_lossy(&stderr)
    );
    assert!(
        !stdout_truncated && !stderr_truncated,
        "{executable} exceeded bounded CLI output"
    );
    Output {
        status,
        stdout,
        stderr,
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

fn success_text(output: Output) -> String {
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout).unwrap()
}

fn assert_retained_help(platform: &str, help: &str) {
    for contract in [
        "--speed NUM/DEN",
        "default 1/1",
        "pitch changes",
        "retained practice/restart/seek/loop",
        "runtime rate changes",
        "unavailable",
        "--start-ns N",
        "--end-ns N",
        "--preroll-ns N",
    ] {
        assert!(
            help.contains(contract),
            "{platform} missing {contract}: {help}"
        );
    }
    if platform == "windows" {
        assert!(help.contains("any positive fixed speed"), "{help}");
        assert!(
            help.contains("Network play retains its existing speed and section restrictions"),
            "{help}"
        );
    } else {
        assert!(help.contains("fixed-speed retained"), "{help}");
        assert!(
            help.contains("network selected-speed play remain unavailable"),
            "{help}"
        );
    }
}

#[test]
fn standalone_and_primary_play_help_publish_retained_capability_and_exclusions() {
    for (platform, executable) in STANDALONE {
        assert_retained_help(
            platform,
            &success_text(invoke(executable, &strings(&["--help"]))),
        );
    }
    let primary = success_text(invoke(PRIMARY, &strings(&["--help"])));
    assert!(primary.contains("play [native options]"), "{primary}");
    assert!(
        primary.contains("Use MODE --help for mode options"),
        "{primary}"
    );
    let host = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    assert_retained_help(
        host,
        &success_text(invoke(PRIMARY, &strings(&["play", "--help"]))),
    );
}

#[cfg(target_os = "linux")]
static SERIAL: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "linux")]
struct Fixture(PathBuf);
#[cfg(target_os = "linux")]
impl Fixture {
    fn new() -> Self {
        Self::with_chart_and_frames(
            b"#TITLE Retained original\n#BPM 60000\n#RANK 2\n#WAV01 key.wav\n#00011:01010101\n",
            480,
        )
    }

    fn with_chart_and_frames(chart: &[u8], samples: u32) -> Self {
        let fixture = loop {
            let directory = std::env::temp_dir().join(format!(
                "beatkernel-retained-cli-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&directory) {
                Ok(()) => break Self(directory),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create CLI fixture: {error}"),
            }
        };
        fs::write(fixture.0.join("original.bms"), chart).unwrap();
        // Original mono 48kHz PCM16; overlap regression uses exactly one frame.
        let bytes = samples * 2;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + bytes).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&48_000u32.to_le_bytes());
        wav.extend_from_slice(&96_000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&bytes.to_le_bytes());
        for _ in 0..samples {
            wav.extend_from_slice(&4096i16.to_le_bytes());
        }
        fs::write(fixture.0.join("key.wav"), wav).unwrap();
        fixture
    }

    fn chart(&self) -> PathBuf {
        self.0.join("original.bms")
    }

    fn linux_args(&self, members: bool, speed: &str) -> Vec<String> {
        let mut args = strings(&[
            "--chart",
            self.chart().to_str().unwrap(),
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
            "--speed",
            speed,
            "--start-ns",
            "1500000",
            "--end-ns",
            "3500000",
            "--preroll-ns",
            "7000000",
            "--bind",
            "11:04",
            "--seconds",
            "1",
        ]);
        if members {
            // Preserve nonsequential original member IDs through primary routing.
            for player in [7, 29] {
                args.extend([
                    "--local-player".into(),
                    format!(
                        "{player}:{}",
                        self.0.join(format!("absent-evdev-{player}")).display()
                    ),
                ]);
            }
        } else {
            args.extend([
                "--evdev".into(),
                self.0.join("absent-evdev").display().to_string(),
            ]);
        }
        args
    }
}

#[cfg(target_os = "linux")]
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(target_os = "linux")]
fn missing_input_after_preparation(output: &Output) -> String {
    assert!(
        !output.status.success(),
        "absent evdev unexpectedly succeeded"
    );
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("Initialization")
            && (error.contains("os error 2")
                || (error.contains("code: 2") && error.contains("kind: NotFound"))),
        "expected explicit absent-input collector refusal after cold preparation: {error}"
    );
    assert!(!error.contains("requires Linux"), "{error}");
    assert!(!error.contains("does not support"), "{error}");
    let feedback = String::from_utf8(output.stdout.clone()).unwrap();
    assert!(
        !feedback.contains("original coordinate anchors="),
        "native startup was claimed: {feedback}"
    );
    assert!(
        !feedback.contains("requested/applied ALSA="),
        "input acquisition was claimed: {feedback}"
    );
    feedback
}

#[cfg(target_os = "linux")]
#[test]
fn retained_finite_solo_and_cohort_prepare_same_original_section_at_each_selected_speed() {
    let fixture = Fixture::new();
    let original_chart = fs::read(fixture.chart()).unwrap();
    let original_wav = fs::read(fixture.0.join("key.wav")).unwrap();
    let mut original_section: Option<String> = None;
    for primary in [false, true] {
        for cohort in [false, true] {
            for (selected, normalized) in [("6/4", "3/2"), ("1/2", "1/2"), ("2/1", "2/1")] {
                let mut args = fixture.linux_args(cohort, selected);
                let executable = if primary {
                    args.insert(0, "play".into());
                    PRIMARY
                } else {
                    env!("CARGO_BIN_EXE_linux_bms")
                };
                let feedback = missing_input_after_preparation(&invoke(executable, &args));
                assert!(
                    feedback.contains(&format!("live playback speed={normalized};")),
                    "{feedback}"
                );
                assert!(
                    feedback.contains("fixed-speed retained practice enabled"),
                    "{feedback}"
                );
                let section = feedback
                    .lines()
                    .find(|line| line.starts_with("prepared practice section="))
                    .expect("actual original section preparation feedback absent");
                assert!(section.contains("start: Timestamp(1500000)"), "{section}");
                assert!(section.contains("excluded_objects: 2"), "{section}");
                if let Some(original) = &original_section {
                    assert_eq!(
                        section,
                        original.as_str(),
                        "speed, roster or primary routing changed original selection"
                    );
                } else {
                    original_section = Some(section.to_owned());
                }
                // No recording, replacement chart, or asset rewrite is permitted
                // during refusal. This test never counts the refusal as playback.
                assert_eq!(fs::read(fixture.chart()).unwrap(), original_chart);
                assert_eq!(fs::read(fixture.0.join("key.wav")).unwrap(), original_wav);
                assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 2);
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn actual_selected_bgm_overlap_reservation_reaches_missing_input_for_all_linux_routes() {
    use beatkernel::audio::{AudioFormat, PcmLimits, SampleId};
    use beatkernel_bms_runtime::{
        native_audio::{required_bgm_overlap, required_bgm_overlap_with_speed},
        replay_speed::ReplaySpeed,
        ChannelPolicy,
    };

    let fixture = Fixture::with_chart_and_frames(
        b"#TITLE Retained BGM envelope\n#BPM 4800000\n#RANK 2\n#WAV01 key.wav\n#00001:0101\n#00011:01\n",
        1,
    );
    // Prove the actual parser/media path yields the literal counterexample;
    // the native invocation below receives these same original files.
    let prepared = beatkernel_bms_runtime::load_prepared_with_seed(
        &fixture.chart(),
        AudioFormat::new(48_000, 1).unwrap(),
        PcmLimits::new(1024, 4096, 8).unwrap(),
        ChannelPolicy::Exact,
        0,
    )
    .unwrap();
    assert_eq!(prepared.bank.get(SampleId(1)).unwrap().frames(), 1);
    assert_eq!(
        prepared
            .bgm_commands
            .iter()
            .map(|command| command.at().as_nanos())
            .collect::<Vec<_>>(),
        [0, 25_000]
    );
    assert_eq!(
        required_bgm_overlap(&prepared.bank, &prepared.bgm_commands).unwrap(),
        1
    );
    assert_eq!(
        required_bgm_overlap_with_speed(
            &prepared.bank,
            &prepared.bgm_commands,
            "3/2".parse::<ReplaySpeed>().unwrap(),
        )
        .unwrap(),
        2
    );
    let original_chart = fs::read(fixture.chart()).unwrap();
    let original_wav = fs::read(fixture.0.join("key.wav")).unwrap();
    for primary in [false, true] {
        for cohort in [false, true] {
            for selected in ["1/1", "3/2", "2/1"] {
                let mut args = fixture.linux_args(cohort, selected);
                for (option, value) in [
                    ("--start-ns", "0"),
                    ("--end-ns", "1000000"),
                    ("--preroll-ns", "0"),
                ] {
                    let position = args.iter().position(|arg| arg == option).unwrap() + 1;
                    args[position] = value.into();
                }
                args.extend(strings(&["--voices", "256"]));
                let executable = if primary {
                    args.insert(0, "play".into());
                    PRIMARY
                } else {
                    env!("CARGO_BIN_EXE_linux_bms")
                };
                let output = invoke(executable, &args);
                assert!(
                    !String::from_utf8_lossy(&output.stderr).contains("Capacity"),
                    "ample total voices refused cold BGM reservation: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let feedback = missing_input_after_preparation(&output);
                if selected == "1/1" {
                    assert!(!feedback.contains("live playback speed="), "{feedback}");
                } else {
                    assert!(
                        feedback.contains(&format!("live playback speed={selected};")),
                        "{feedback}"
                    );
                }
                assert!(
                    feedback.contains("prepared practice section="),
                    "{feedback}"
                );
                assert_eq!(
                    feedback.contains("fixed-speed retained practice enabled"),
                    selected != "1/1",
                    "{feedback}"
                );
                assert_eq!(fs::read(fixture.chart()).unwrap(), original_chart);
                assert_eq!(fs::read(fixture.0.join("key.wav")).unwrap(), original_wav);
                assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 2);
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn foreign_selected_finite_setup_reaches_platform_guard_while_invalid_region_refuses_in_parser() {
    let fixture = Fixture::new();
    for (platform, executable) in STANDALONE.into_iter().filter(|(name, _)| *name != "linux") {
        let mut args = strings(&[
            "--chart",
            fixture.chart().to_str().unwrap(),
            "--speed",
            "6/4",
            "--start-ns",
            "1500000",
            "--end-ns",
            "3500000",
            "--preroll-ns",
            "7000000",
            "--bind",
            "11:04",
        ]);
        if platform == "windows" {
            args.extend(strings(&["--device", "absent-output", "--mode", "shared"]));
        } else {
            args.extend(strings(&[
                "--device",
                "1",
                "--keyboard-registry",
                "1",
                "--rate",
                "48000",
                "--channels",
                "1",
                "--buffer-frames",
                "256",
            ]));
        }
        let valid = invoke(executable, &args);
        assert!(!valid.status.success());
        let expected_guard = if platform == "windows" {
            "requires Windows"
        } else {
            "requires macOS"
        };
        assert!(String::from_utf8_lossy(&valid.stderr).contains(expected_guard));
        let feedback = String::from_utf8(valid.stdout).unwrap();
        assert!(feedback.contains("live playback speed=3/2;"), "{feedback}");
        assert!(
            !feedback.contains("prepared practice section="),
            "foreign native preparation claimed: {feedback}"
        );

        // The same actual retained invocation with a reversed original region
        // must fail before even the platform guard or selected-speed feedback.
        let endpoint = args.iter().position(|arg| arg == "--end-ns").unwrap() + 1;
        args[endpoint] = "1000000".into();
        let invalid = invoke(executable, &args);
        assert!(!invalid.status.success());
        assert!(invalid.stdout.is_empty());
        let error = String::from_utf8_lossy(&invalid.stderr);
        assert!(
            !error.contains(expected_guard),
            "platform acquisition preceded invalid region: {error}"
        );
        assert!(
            error.to_ascii_lowercase().contains("end"),
            "missing finite-region diagnostic: {error}"
        );
    }
}
