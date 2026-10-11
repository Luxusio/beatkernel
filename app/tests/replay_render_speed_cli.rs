#[path = "support/replay_speed.rs"]
mod fixture;

use beatkernel::{
    replay::codec::encode_replay,
    time::{Duration, Timestamp},
};
use beatkernel_bms_runtime::{gauge::GaugeProfile, replay_audio::plan_audio};
use fixture::*;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

const TEXT: &str = "#BPM 60\n#WAV01 key.wav\n#WAV02 bgm.wav\n#00011:00010000\n#00001:02\n";
const BGM: &[f32] = &[0.0625, 0.125, 0.1875, 0.25, 0.3125, 0.375, 0.4375, 0.5];
static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "beatkernel-speed-cli-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wav(path: &Path, values: &[f32]) {
    // Original signed PCM16 WAV; every chosen value is exactly representable.
    let data = (values.len() * 2) as u32;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&4u32.to_le_bytes());
    b.extend_from_slice(&8u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for &v in values {
        b.extend_from_slice(&((v * 32768.0) as i16).to_le_bytes());
    }
    fs::write(path, b).unwrap();
}
fn setup(
    tree: &Tree,
) -> (
    beatkernel_bms_runtime::PreparedBms,
    beatkernel::replay::codec::ReplayFile,
) {
    fs::write(tree.path("chart.bms"), TEXT).unwrap();
    wav(&tree.path("key.wav"), &[-0.125]);
    wav(&tree.path("bgm.wav"), BGM);
    let p = prepared(TEXT, 4, &[(1, &[-0.125]), (2, BGM)]);
    let file = captured(&p, 0, Some(4_000_000_001));
    fs::write(
        tree.path("session.bkr"),
        encode_replay(&file, limits()).unwrap(),
    )
    .unwrap();
    (p, file)
}
fn args(tree: &Tree, output: &str) -> Vec<String> {
    vec![
        "--chart".into(),
        tree.path("chart.bms").display().to_string(),
        "--replay".into(),
        tree.path("session.bkr").display().to_string(),
        "--output".into(),
        tree.path(output).display().to_string(),
        "--seconds".into(),
        "5".into(),
        "--rate".into(),
        "4".into(),
        "--channels".into(),
        "1".into(),
        "--preroll-ns".into(),
        "0".into(),
    ]
}
fn run(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_render_replay_bms"))
        .args(args)
        .output()
        .unwrap()
}
fn extra(args: &mut Vec<String>, items: &[&str]) {
    args.extend(items.iter().map(|s| s.to_string()));
}
fn replace(args: &mut [String], flag: &str, value: &str) {
    let i = args.iter().position(|s| s == flag).unwrap();
    args[i + 1] = value.into();
}
fn success(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}
fn failure(out: &Output) -> String {
    assert!(!out.status.success());
    assert!(
        out.stdout.is_empty(),
        "failed invocation printed success: {:?}",
        out.stdout
    );
    String::from_utf8(out.stderr.clone()).unwrap()
}
fn floats(path: &Path) -> Vec<f32> {
    let b = fs::read(path).unwrap();
    assert_eq!(b.len() % 4, 0);
    b.chunks_exact(4)
        .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
        .collect()
}

#[test]
fn actual_binary_rational_pcm_normalized_summary_and_original_judge_identity() {
    let tree = Tree::new();
    let (p, file) = setup(&tree);
    let original_bytes = fs::read(tree.path("session.bkr")).unwrap();
    let logical = plan_audio(&p, file, limits(), point(0), Duration::ZERO).unwrap();
    let cases = [
        (
            "1/2",
            vec![
                0.0625, 0.09375, 0.125, 0.15625, 0.1875, 0.21875, 0.25, 0.28125, 0.1875, 0.21875,
                0.375, 0.40625, 0.4375, 0.46875, 0.5, 0.5,
            ],
        ),
        (
            "1/1",
            vec![0.0625, 0.125, 0.1875, 0.25, 0.1875, 0.375, 0.4375, 0.5],
        ),
        ("2/1", vec![0.0625, 0.1875, 0.1875, 0.4375]),
        ("3/2", vec![0.0625, 0.15625, 0.25, 0.21875, 0.4375, 0.5]),
    ];
    for (i, (speed, mut expected)) in cases.into_iter().enumerate() {
        expected.resize(20, 0.0);
        for block in [1, 3, 32] {
            let name = format!("speed-{i}-{block}.pcm");
            let mut a = args(&tree, &name);
            extra(
                &mut a,
                &["--speed", speed, "--block-frames", &block.to_string()],
            );
            let summary = success(&run(&a));
            assert_eq!(floats(&tree.path(&name)), expected, "{speed} block {block}");
            assert!(summary.contains(&format!("speed={speed}")), "{summary}");
            assert!(summary.contains("frames=20"));
            assert!(summary.contains("full_replay_results=1"));
            assert!(summary.contains("full_replay_hits=1"));
            assert!(summary.contains(&format!(
                "final_judge_hash={:#018x}",
                logical.final_judge_hash
            )));
            assert!(summary.contains(if speed == "1/1" {
                "commands_admitted=2"
            } else {
                "commands_admitted=3"
            }));
        }
    }
    assert_eq!(fs::read(tree.path("session.bkr")).unwrap(), original_bytes);
}

#[test]
fn default_explicit_and_reduced_normal_pcm_and_normalized_non_normal_are_compatible() {
    let tree = Tree::new();
    setup(&tree);
    let mut baseline = None;
    for (i, speed) in [None, Some("1/1"), Some("8/8"), Some("0002/0002")]
        .into_iter()
        .enumerate()
    {
        let name = format!("normal-{i}.pcm");
        let mut a = args(&tree, &name);
        if let Some(speed) = speed {
            extra(&mut a, &["--speed", speed]);
        }
        let summary = success(&run(&a));
        assert!(summary.contains("speed=1/1"));
        assert!(summary.contains("commands_admitted=2"));
        let bytes = fs::read(tree.path(&name)).unwrap();
        if let Some(ref b) = baseline {
            assert_eq!(&bytes, b);
        } else {
            baseline = Some(bytes);
        }
    }
    let mut a = args(&tree, "reduced.pcm");
    extra(&mut a, &["--speed", "6/4"]);
    assert!(success(&run(&a)).contains("speed=3/2"));
    let mut b = args(&tree, "direct.pcm");
    extra(&mut b, &["--speed", "3/2"]);
    success(&run(&b));
    assert_eq!(
        fs::read(tree.path("reduced.pcm")).unwrap(),
        fs::read(tree.path("direct.pcm")).unwrap()
    );
}

#[test]
fn output_preroll_seconds_cutoff_and_device_hz_remain_separate_from_speed() {
    let tree = Tree::new();
    setup(&tree);
    let mut a = args(&tree, "preroll.pcm");
    replace(&mut a, "--seconds", "2");
    replace(&mut a, "--preroll-ns", "250000000");
    extra(&mut a, &["--speed", "2/1"]);
    let summary = success(&run(&a));
    assert!(summary.contains("frames=8"));
    assert_eq!(
        floats(&tree.path("preroll.pcm")),
        [0.0, 0.0625, 0.1875, 0.1875, 0.4375, 0.0, 0.0, 0.0]
    );
    let mut b = args(&tree, "cutoff.pcm");
    replace(&mut b, "--seconds", "1");
    replace(&mut b, "--preroll-ns", "1000000000");
    extra(&mut b, &["--speed", "2/1"]);
    let summary = success(&run(&b));
    assert_eq!(floats(&tree.path("cutoff.pcm")), [0.0; 4]);
    assert!(summary.contains("commands_admitted=1"));
    assert!(summary.contains("full_replay_hits=1"));
    let mut c = args(&tree, "hz.pcm");
    replace(&mut c, "--rate", "8");
    replace(&mut c, "--seconds", "2");
    extra(&mut c, &["--speed", "2/1"]);
    assert!(success(&run(&c)).contains("frames=16"));
    assert_eq!(fs::metadata(tree.path("hz.pcm")).unwrap().len(), 16 * 4);
}

#[test]
fn invalid_ratios_and_duplicate_flags_fail_before_replay_chart_or_output_access() {
    let tree = Tree::new();
    // None of the input paths exists. Parsing must win over File::open errors.
    for (i, value) in [
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
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("invalid-{i}.pcm");
        let mut a = args(&tree, &name);
        extra(&mut a, &["--speed", value]);
        let err = failure(&run(&a));
        assert!(
            !err.contains("No such file") && !err.contains("os error 2"),
            "filesystem access preceded speed validation: {err}"
        );
        assert!(!tree.path(&name).exists());
    }
    let mut a = args(&tree, "duplicate.pcm");
    extra(&mut a, &["--speed", "1/1", "--speed", "2/1"]);
    assert!(failure(&run(&a)).contains("duplicate"));
    assert!(!tree.path("duplicate.pcm").exists());
    let mut b = args(&tree, "missing.pcm");
    extra(&mut b, &["--speed"]);
    let err = failure(&run(&b));
    assert!(!err.contains("os error 2"));
    assert!(!tree.path("missing.pcm").exists());
}

#[test]
fn help_explains_fractional_pitch_changing_speed_and_device_rate() {
    let summary = success(&run(&["--help".into()]));
    assert!(summary.contains("--speed NUM/DEN"));
    assert!(summary.contains("1/1"));
    assert!(summary.to_ascii_lowercase().contains("pitch"));
    assert!(summary.contains("--rate HZ"));
}

#[test]
fn output_collision_preserves_bytes_and_real_origin_queue_refusal_leaves_empty_new_file() {
    let tree = Tree::new();
    setup(&tree);
    fs::write(tree.path("existing.pcm"), b"preserve-me").unwrap();
    let mut a = args(&tree, "existing.pcm");
    extra(&mut a, &["--speed", "2/1"]);
    failure(&run(&a));
    assert_eq!(fs::read(tree.path("existing.pcm")).unwrap(), b"preserve-me");
    // The original BGM is at frame zero, so added SetRate consumes the one
    // available admission before Play: this is a real original queue failure.
    let mut b = args(&tree, "refusal.pcm");
    extra(&mut b, &["--speed", "2/1", "--command-capacity", "1"]);
    failure(&run(&b));
    assert_eq!(fs::metadata(tree.path("refusal.pcm")).unwrap().len(), 0);
    let mut c = args(&tree, "normal-small-queue.pcm");
    extra(&mut c, &["--command-capacity", "1"]);
    success(&run(&c));
}

#[test]
fn finite_and_nondefault_policy_metadata_are_still_refused_by_product_cli() {
    let tree = Tree::new();
    let (p, _) = setup(&tree);
    let finite = captured_at(
        &p,
        0,
        Timestamp::ZERO,
        Some(Timestamp::from_nanos(4_000_000_000)),
        Some(4_000_000_000),
    );
    let gauge = GaugeProfile::new(1, 0, 0, -1, true, vec![]).unwrap();
    let policy = capture_events(
        &p,
        0,
        Timestamp::ZERO,
        None,
        &gauge,
        &[],
        Some(4_000_000_001),
    );
    for (i, file) in [finite, policy].into_iter().enumerate() {
        fs::write(
            tree.path("session.bkr"),
            encode_replay(&file, limits()).unwrap(),
        )
        .unwrap();
        for speed in ["1/1", "2/1"] {
            let name = format!("unsupported-{i}-{}.pcm", speed.replace('/', "-"));
            let mut a = args(&tree, &name);
            extra(&mut a, &["--speed", speed]);
            failure(&run(&a));
            if tree.path(&name).exists() {
                assert_eq!(fs::metadata(tree.path(&name)).unwrap().len(), 0);
            }
        }
    }
}
