//! Shipped encoding routes. Null output and absent evdev prove cold preparation
//! and explicit refusal; they never establish physical gameplay or sound timing.
#![cfg(target_os = "linux")]

#[path = "support/replay_speed.rs"]
mod capture;

use std::{
    fs,
    io::Read,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const PRIMARY: &str = env!("CARGO_BIN_EXE_beatkernel-bms-runtime");
const LIVE: &str = env!("CARGO_BIN_EXE_linux_bms");
const REPLAY: &str = env!("CARGO_BIN_EXE_play_replay_bms");
const TOKENS: [&str; 4] = ["float32", "pcm16", "pcm24", "pcm32"];
const CHART: &str =
    "#TITLE Encoding original\n#BPM 60\n#WAV01 key.wav\n#00011:00010000\n#00001:01\n";
static SERIAL: AtomicU64 = AtomicU64::new(0);

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn drain(mut pipe: impl Read) -> (Vec<u8>, bool) {
    let mut kept = Vec::new();
    let mut truncated = false;
    let mut chunk = [0; 4096];
    loop {
        let count = match pipe.read(&mut chunk) {
            Ok(0) => return (kept, truncated),
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("CLI output read: {error}"),
        };
        let retain = count.min(1024 * 1024 - kept.len());
        kept.extend_from_slice(&chunk[..retain]);
        truncated |= retain < count;
    }
}

fn invoke(executable: &str, args: &[String]) -> Output {
    invoke_with_environment(executable, args, false)
}

fn invoke_with_environment(executable: &str, args: &[String], headless: bool) -> Output {
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if headless {
        // Invalid player options must fail during parsing. A regression must
        // also fail this test, while being unable to create an X11/Wayland window.
        command.env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
    }
    let mut process = Process(command.spawn().unwrap());
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
        "CLI deadline exceeded: {}",
        String::from_utf8_lossy(&stderr)
    );
    assert!(!stdout_truncated && !stderr_truncated, "CLI output limit");
    Output {
        status,
        stdout,
        stderr,
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

fn route(replay: bool, primary: bool, mut args: Vec<String>) -> (&'static str, Vec<String>) {
    if primary {
        args.insert(0, if replay { "play-replay" } else { "play" }.into());
        (PRIMARY, args)
    } else {
        (if replay { REPLAY } else { LIVE }, args)
    }
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let fixture = loop {
            let path = std::env::temp_dir().join(format!(
                "beatkernel-encoding-cli-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => break Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("CLI fixture: {error}"),
            }
        };
        fs::write(fixture.chart(), CHART).unwrap();
        let samples = [8192i16, -16384, 0, 16384].repeat(16);
        let bytes = u32::try_from(samples.len() * 2).unwrap();
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
        for sample in samples {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        fs::write(fixture.0.join("key.wav"), wav).unwrap();
        fixture
    }

    fn chart(&self) -> PathBuf {
        self.0.join("original.bms")
    }

    fn live_args(&self, roster: usize) -> Vec<String> {
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
            "--bind",
            "11:04",
            "--preroll-ns",
            "0",
            "--seconds",
            "1",
        ]);
        let absent = |id| {
            self.0
                .join(format!("absent-evdev-{id}"))
                .display()
                .to_string()
        };
        match roster {
            0 => args.extend(["--evdev".into(), absent(1)]),
            1 => {
                for id in [1, 2] {
                    args.extend(["--local-input".into(), absent(id)]);
                }
            }
            2 => {
                for id in [7, 29] {
                    args.extend(["--local-player".into(), format!("{id}:{}", absent(id))]);
                }
            }
            _ => unreachable!(),
        }
        args
    }

    fn replay_args(&self) -> Vec<String> {
        strings(&[
            "--chart",
            self.chart().to_str().unwrap(),
            "--replay",
            self.0.join("original.bkr").to_str().unwrap(),
            "--device",
            "null",
            "--rate",
            "48000",
            "--channels",
            "1",
            "--period-frames",
            "128",
            "--buffer-frames",
            "256",
            "--preroll-ns",
            "0",
            "--seconds",
            "1",
        ])
    }

    fn originals(&self) -> (Vec<u8>, Vec<u8>) {
        (
            fs::read(self.chart()).unwrap(),
            fs::read(self.0.join("key.wav")).unwrap(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn output_encoding_shipped_help_exposes_four_exact_tokens_and_default() {
    for replay in [false, true] {
        for primary in [false, true] {
            let (executable, args) = route(replay, primary, strings(&["--help"]));
            let output = invoke(executable, &args);
            assert!(output.status.success());
            assert!(output.stderr.is_empty());
            let help = String::from_utf8(output.stdout).unwrap();
            assert!(
                help.contains("--encoding float32|pcm16|pcm24|pcm32"),
                "{help}"
            );
            assert!(
                help.contains("encoding float32") || help.contains("default float32"),
                "{help}"
            );
        }
    }
}

#[cfg(feature = "desktop")]
#[test]
fn output_encoding_primary_player_rejects_invalid_cli_before_profile_catalog_or_gui_io() {
    use beatkernel_bms_runtime::{
        presentation_settings::PresentationSettings,
        settings::{NativeSettings, SettingsHost},
        settings_profile::{PlayerProfile, save_player_profile, save_profile},
    };
    let fixture = Fixture::new();
    let mut saved = fixture.live_args(0);
    saved.extend(strings(&["--encoding", "pcm24"]));
    let native = NativeSettings::from_args(&saved, SettingsHost::Linux).unwrap();
    save_profile(&fixture.0.join("v1.bkp"), &native, SettingsHost::Linux).unwrap();
    save_player_profile(
        &fixture.0.join("v2.bkp"),
        &PlayerProfile {
            native,
            presentation: PresentationSettings::default(),
        },
        SettingsHost::Linux,
    )
    .unwrap();
    for version in ["v1.bkp", "v2.bkp", "absent.bkp"] {
        for invalid in ["", " ", "PCM16", "pcm24 ", "pcm8"] {
            let args = strings(&[
                "player",
                "--chart",
                fixture.0.join("absent-chart.bms").to_str().unwrap(),
                "--profile",
                fixture.0.join(version).to_str().unwrap(),
                "--title-font",
                fixture.0.join("absent-font.ttf").to_str().unwrap(),
                "--encoding",
                invalid,
            ]);
            let output = invoke_with_environment(PRIMARY, &args, true);
            assert!(
                !output.status.success(),
                "player accepted {invalid:?}/{version}"
            );
            assert!(output.stdout.is_empty(), "startup metadata before refusal");
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(
                error.contains("encoding"),
                "wrong refusal for {invalid:?}/{version}: {error}"
            );
            assert!(
                !error.contains("os error 2") && !error.contains("NotFound"),
                "{error}"
            );
            assert!(
                !error.contains("EventLoop")
                    && !error.contains("DISPLAY")
                    && !error.contains("WAYLAND"),
                "{error}"
            );
            assert!(
                !error.contains("ALSA") && !error.contains("Initialization"),
                "{error}"
            );
        }
    }
}

#[test]
fn output_encoding_shipped_strict_refusal_precedes_chart_replay_and_device_io() {
    let fixture = Fixture::new();
    for replay in [false, true] {
        for primary in [false, true] {
            let mut base = if replay {
                fixture.replay_args()
            } else {
                fixture.live_args(0)
            };
            base[1] = fixture.0.join("absent-chart.bms").display().to_string();
            for suffix in [
                vec!["--encoding", ""],
                vec!["--encoding", " "],
                vec!["--encoding", "pcm8"],
                vec!["--encoding", "PCM16"],
                vec!["--encoding", "pcm24 "],
                vec!["--encoding", "pcm24-in-32"],
                vec!["--encoding", "pcm16", "--encoding", "pcm16"],
                vec!["--encoding", "float32", "--encoding", "pcm32"],
                vec!["--encoding"],
            ] {
                let mut args = base.clone();
                args.extend(strings(&suffix));
                let (executable, args) = route(replay, primary, args);
                let output = invoke(executable, &args);
                assert!(!output.status.success(), "accepted {suffix:?}");
                let error = String::from_utf8(output.stderr).unwrap();
                assert!(
                    error.contains("encoding")
                        || error.contains("option requires a value")
                        || error.contains("every option requires a value")
                        || error.contains("settings require at most 128 flag/value pairs"),
                    "wrong refusal for {suffix:?}: {error}"
                );
                assert!(
                    !error.contains("os error 2") && !error.contains("NotFound"),
                    "{error}"
                );
                assert!(
                    !error.contains("Initialization") && !error.contains("ALSA"),
                    "{error}"
                );
                assert!(
                    !String::from_utf8_lossy(&output.stdout).contains("prepared practice section=")
                );
            }
        }
    }
}

#[test]
fn output_encoding_shipped_valid_solo_and_local_cold_paths_reach_explicit_absent_input() {
    let fixture = Fixture::new();
    let originals = fixture.originals();
    let mut original_section: Option<String> = None;
    for primary in [false, true] {
        for roster in 0..3 {
            for speed in ["1/1", "3/2"] {
                for token in TOKENS {
                    let mut args = fixture.live_args(roster);
                    args.extend(strings(&["--speed", speed, "--encoding", token]));
                    let (executable, args) = route(false, primary, args);
                    let output = invoke(executable, &args);
                    assert!(!output.status.success(), "absent input succeeded: {token}");
                    let error = String::from_utf8_lossy(&output.stderr);
                    assert!(
                        error.contains("Initialization")
                            && (error.contains("os error 2")
                                || (error.contains("code: 2") && error.contains("NotFound"))),
                        "{token}/{roster}/{speed}: {error}"
                    );
                    let feedback = String::from_utf8(output.stdout).unwrap();
                    let section = feedback
                        .lines()
                        .find(|line| line.starts_with("prepared practice section="))
                        .expect("actual chart/WAV preparation absent");
                    if let Some(original) = &original_section {
                        assert_eq!(
                            section,
                            original.as_str(),
                            "native encoding changed original chart selection"
                        );
                    } else {
                        original_section = Some(section.to_owned());
                    }
                    assert_eq!(
                        feedback.contains("live playback speed=3/2;"),
                        speed == "3/2"
                    );
                    assert!(
                        !feedback.contains("original coordinate anchors="),
                        "{feedback}"
                    );
                    assert!(!feedback.contains("requested/applied ALSA="), "{feedback}");
                    assert_eq!(fixture.originals(), originals);
                    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 2);
                }
            }
        }
    }
}

#[test]
fn output_encoding_primary_and_standalone_replay_accept_tokens_before_missing_recording() {
    let fixture = Fixture::new();
    for primary in [false, true] {
        for token in TOKENS {
            let mut args = fixture.replay_args();
            args.extend(strings(&["--encoding", token]));
            let (executable, args) = route(true, primary, args);
            let output = invoke(executable, &args);
            assert!(!output.status.success());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(
                error.contains("os error 2") || error.contains("NotFound"),
                "{token}: {error}"
            );
            assert!(
                !error.contains("encoding") && !error.contains("ALSA"),
                "{error}"
            );
            assert!(output.stdout.is_empty());
        }
    }
}

#[test]
#[ignore = "opted real recorded CLI playback: BEATKERNEL_TEST_ALSA_DEVICE=null; no acoustic proof"]
fn output_encoding_actual_recorded_null_cli_all_four_preserve_original_capture_and_source_bytes() {
    assert_eq!(
        std::env::var("BEATKERNEL_TEST_ALSA_DEVICE").as_deref(),
        Ok("null")
    );
    use beatkernel::{replay::codec::encode_replay, time::Timestamp};
    let fixture = Fixture::new();
    let originals = fixture.originals();
    let pcm = [0.25, -0.5, 0.0, 0.5].repeat(16);
    let prepared = capture::prepared(CHART, 48_000, &[(1, &pcm)]);
    let recording = capture::captured_at(
        &prepared,
        0,
        Timestamp::ZERO,
        Some(Timestamp::from_nanos(4_000_000_000)),
        Some(4_000_000_000),
    );
    let bytes = encode_replay(&recording, capture::limits()).unwrap();
    let replay_path = fixture.0.join("original.bkr");
    fs::write(&replay_path, &bytes).unwrap();
    for primary in [false, true] {
        for (token, encoding) in [
            ("float32", "encoding: Float32"),
            (
                "pcm16",
                "encoding: Pcm { container_bits: 16, valid_bits: 16 }",
            ),
            (
                "pcm24",
                "encoding: Pcm { container_bits: 24, valid_bits: 24 }",
            ),
            (
                "pcm32",
                "encoding: Pcm { container_bits: 32, valid_bits: 32 }",
            ),
        ] {
            let mut args = fixture.replay_args();
            args.extend(strings(&[
                "--encoding",
                token,
                "--output-rate",
                "44100",
                "--speed",
                "2/1",
            ]));
            let (executable, args) = route(true, primary, args);
            let output = invoke(executable, &args);
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(output.status.success(), "{token}/{primary}: {error}");
            let feedback = String::from_utf8(output.stdout).unwrap();
            for expected in [
                format!("ALSA requested encoding={token}; applied exact target output="),
                encoding.into(),
                "sample_rate: 44100".into(),
                "reconstructed logical replay:".into(),
                "speed=2/1".into(),
                "recovered complete converter=true".into(),
                "seconds=Some(1)".into(),
            ] {
                assert!(
                    feedback.contains(&expected),
                    "missing {expected}: {feedback}"
                );
            }
            let admission = feedback
                .lines()
                .find(|line| line.starts_with("final command admission config="))
                .expect("actual immutable-source admission diagnostics");
            assert!(admission.contains("sample_rate: 48000"), "{admission}");
            assert_eq!(fs::read(&replay_path).unwrap(), bytes);
            assert_eq!(encode_replay(&recording, capture::limits()).unwrap(), bytes);
            assert_eq!(fixture.originals(), originals);
            assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 3);
        }
    }
}
