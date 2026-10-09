//! Native adapter contracts. Ignored tests below execute actual external codecs.
#![cfg(not(target_arch = "wasm32"))]

use std::{
    path::PathBuf,
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

use beatkernel::time::Timestamp;
use beatkernel_bms::{BgaChannel, ImageId};
use beatkernel_bms_runtime::{
    video::{
        VideoDecodeEvent, VideoDecoderPort, VideoFrameAdmission, VideoFrameLimits, VideoFrameQueue,
        VideoSessionKey, VideoTimeBase,
    },
    video_assets::{VideoAssetLimits, VideoResource, VideoResourceDescriptor, VideoTransform},
    video_native::{finish_rgba, FfmpegDecoder, FfmpegDecoderConfig, ShowInfoParser},
};

fn ns(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn session(generation: u64) -> VideoSessionKey {
    VideoSessionKey {
        content: 71,
        generation,
        image: ImageId(3),
        channel: BgaChannel::Base,
        activated_at: ns(900_000_000),
        ordinal: Some(4),
    }
}
fn config_line(base: &str) -> String {
    format!("[Parsed_showinfo_1 @ 0x123] config in time_base: {base}, frame_rate: 25/1")
}
fn frame_line(ordinal: &str, pts: &str, format: &str, extent: &str) -> String {
    // pts_time deliberately disagrees with integer PTS: floats are never authoritative.
    format!("[Parsed_showinfo_1 @ 0x123] n: {ordinal} pts: {pts} pts_time:123.456 pos:-1 fmt:{format} sar:1/1 s:{extent} i:P iskey:1 type:I checksum:00000000 plane_checksum:[00000000] mean:[0] stdev:[0.0]")
}
fn parser() -> ShowInfoParser {
    ShowInfoParser::new(VideoAssetLimits::default())
}

#[test]
fn filter_timebase_and_integer_pts_are_authoritative() {
    let mut parser = parser();
    assert!(parser
        .parse_line("ordinary ffmpeg diagnostic")
        .unwrap()
        .is_none());
    assert!(parser
        .parse_line(&config_line("1/90000"))
        .unwrap()
        .is_none());
    let metadata = parser
        .parse_line(&frame_line("0", "450000", "rgba", "2x3"))
        .unwrap()
        .expect("showinfo frame metadata");
    assert_eq!(
        (
            metadata.ordinal,
            metadata.pts,
            metadata.width,
            metadata.height,
            metadata.byte_len
        ),
        (0, 450000, 2, 3, 24)
    );
    assert_eq!(metadata.time_base, VideoTimeBase::new(1, 90000).unwrap());
    assert_eq!(
        metadata.time_base.timestamp(450001, metadata.pts).unwrap(),
        ns(11_111)
    );
    let next = parser
        .parse_line(&frame_line("1", "453601", "rgba", "2x3"))
        .unwrap()
        .unwrap();
    assert_eq!(
        next.time_base.timestamp(next.pts, metadata.pts).unwrap(),
        ns(40_011_111)
    );
}

#[test]
fn filter_metadata_is_required_before_rgba_allocation() {
    let mut parser = parser();
    assert!(parser
        .parse_line(&frame_line("0", "0", "rgba", "2x2"))
        .is_err());
    for base in ["0/1000", "1/0", "4294967296/1", "garbage"] {
        assert!(
            self::parser().parse_line(&config_line(base)).is_err(),
            "base {base}"
        );
    }
}

#[test]
fn frame_ordinals_cannot_skip_repeat_or_start_after_zero() {
    for wrong in ["1", "-1", "18446744073709551616"] {
        let mut parser = parser();
        parser.parse_line(&config_line("1/1000")).unwrap();
        assert!(parser
            .parse_line(&frame_line(wrong, "0", "rgba", "2x2"))
            .is_err());
    }
    for wrong in ["0", "2"] {
        let mut parser = parser();
        parser.parse_line(&config_line("1/1000")).unwrap();
        parser
            .parse_line(&frame_line("0", "5000", "rgba", "2x2"))
            .unwrap();
        assert!(parser
            .parse_line(&frame_line(wrong, "5040", "rgba", "2x2"))
            .is_err());
    }
}

#[test]
fn unsupported_format_invalid_extent_and_pts_overflow_are_explicit() {
    for (pts, format, extent) in [
        ("0", "yuv420p", "2x2"),
        ("0", "rgba", "0x2"),
        ("0", "rgba", "16385x1"),
        ("0", "rgba", "16384x16384"),
        ("9223372036854775808", "rgba", "2x2"),
        ("NOPTS", "rgba", "2x2"),
    ] {
        let mut parser = parser();
        parser.parse_line(&config_line("1/1000")).unwrap();
        assert!(
            parser
                .parse_line(&frame_line("0", pts, format, extent))
                .is_err(),
            "must reject pts={pts} fmt={format} size={extent}"
        );
    }
}

#[test]
fn rgba_completion_rejects_truncation_and_extra_bytes() {
    for length in [0, 15, 17] {
        let mut parser = parser();
        parser.parse_line(&config_line("1/1000")).unwrap();
        let metadata = parser
            .parse_line(&frame_line("0", "5000", "rgba", "2x2"))
            .unwrap()
            .unwrap();
        assert!(finish_rgba(metadata, vec![0; length]).is_err());
    }
    let mut parser = parser();
    parser.parse_line(&config_line("1/1000")).unwrap();
    let metadata = parser
        .parse_line(&frame_line("0", "5000", "rgba", "2x2"))
        .unwrap()
        .unwrap();
    let pixels = vec![7, 8, 9, 255].repeat(4);
    let image = finish_rgba(metadata, pixels.clone()).unwrap();
    assert_eq!((image.width(), image.height()), (2, 2));
    assert_eq!(image.pixels(), pixels.as_slice());
}

#[test]
fn signed_integer_pts_keep_negative_preroll_and_large_source_origin() {
    let mut parser = parser();
    parser.parse_line(&config_line("1/1000000000")).unwrap();
    let first = parser
        .parse_line(&frame_line("0", "-9223372036854775808", "rgba", "1x1"))
        .unwrap()
        .unwrap();
    let second = parser
        .parse_line(&frame_line("1", "-9223372036854775803", "rgba", "1x1"))
        .unwrap()
        .unwrap();
    assert_eq!(first.pts, i64::MIN);
    assert_eq!(
        second.time_base.timestamp(second.pts, first.pts).unwrap(),
        ns(5)
    );
}

fn transform() -> VideoTransform {
    VideoTransform {
        crop: None,
        canvas: None,
        keyed: false,
    }
}

#[test]
fn unavailable_explicit_executable_reports_reason_and_preserves_static_capability() {
    let missing =
        std::env::temp_dir().join(format!("beatkernel-missing-decoder-{}", std::process::id()));
    assert!(
        !missing.exists(),
        "test sentinel must not identify a real executable"
    );
    let mut decoder = FfmpegDecoder::open(
        VideoResourceDescriptor {
            data: VideoResource::File(missing.with_extension("mkv")),
            encoded_bytes: 11,
        },
        transform(),
        FfmpegDecoderConfig {
            executable: Some(missing),
            ..Default::default()
        },
    )
    .unwrap();
    let capability = decoder.capabilities();
    assert!(!capability.available);
    assert!(capability.reason.is_some_and(|reason| !reason.is_empty()));
    assert!(decoder.request(session(1), ns(0)).is_err());
    assert!(decoder.try_next().is_none());
    decoder.shutdown();
    decoder.join().unwrap();
}

#[test]
fn working_memory_budget_is_separate_and_invalid_zero_rejects_before_decode() {
    assert_eq!(
        FfmpegDecoderConfig::default().max_working_bytes,
        256 * 1024 * 1024
    );
    let missing = std::env::temp_dir().join(format!(
        "beatkernel-invalid-budget-decoder-{}",
        std::process::id()
    ));
    assert!(!missing.exists());
    let result = FfmpegDecoder::open(
        VideoResourceDescriptor {
            data: VideoResource::File(missing.with_extension("mkv")),
            encoded_bytes: 11,
        },
        transform(),
        FfmpegDecoderConfig {
            executable: Some(missing),
            max_working_bytes: 0,
            ..Default::default()
        },
    );
    assert!(
        result.is_err(),
        "invalid working budget must fail before child IO, even with no installed tool"
    );
}

#[cfg(target_os = "linux")]
fn stalled_probe_fixture(label: &str) -> (Fixture, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let directory = std::env::temp_dir().join(format!(
        "beatkernel-stalled-probe-{}-{label}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let ffmpeg = directory.join("ffmpeg");
    let probe = directory.join("ffprobe");
    std::fs::write(&ffmpeg, "#!/bin/sh\nexit 0\n").unwrap();
    // exec makes the sleeping process the probe itself, with no unowned
    // descendant. Its stdout is already EOF while its lifetime continues.
    std::fs::write(
        &probe,
        "#!/bin/sh\nprintf '%s' \"$$\" > \"$0.pid\"\nexec 1>&-\nexec /bin/sleep 8\n",
    )
    .unwrap();
    for executable in [&ffmpeg, &probe] {
        std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let movie = directory.join("unused.mkv");
    std::fs::write(&movie, []).unwrap();
    let pid = directory.join("ffprobe.pid");
    (
        Fixture {
            directory,
            movie,
            ffmpeg,
        },
        pid,
    )
}

#[cfg(target_os = "linux")]
fn wait_probe_pid(marker: &std::path::Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(text) = std::fs::read_to_string(marker) {
            if let Ok(pid) = text.parse() {
                return pid;
            }
        }
        assert!(Instant::now() < deadline, "probe wrapper did not start");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn probe_deadline_includes_exit_after_stdout_eof_and_reaps_child() {
    let (fixture, marker) = stalled_probe_fixture("deadline");
    let mut decoder = open_fixture(&fixture);
    let key = session(1);
    let started = Instant::now();
    decoder.request(key, ns(0)).unwrap();
    let pid = wait_probe_pid(&marker);
    loop {
        if let Some(event) = decoder.try_next() {
            match event {
                VideoDecodeEvent::End { session, end } => {
                    assert_eq!(session, key);
                    assert_eq!(end, None);
                    break;
                }
                VideoDecodeEvent::Failed { reason, .. } => panic!("fallback failed: {reason}"),
                _ => panic!("empty fallback must not emit video frames"),
            }
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "fallback deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    decoder.join().unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(7),
        "five-second probe deadline must also bound the wait after pipe EOF"
    );
    assert!(
        !PathBuf::from(format!("/proc/{pid}")).exists(),
        "probe must be reaped"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn probe_shutdown_after_stdout_eof_kills_and_reaps_without_waiting_for_exit() {
    let (fixture, marker) = stalled_probe_fixture("shutdown");
    let mut decoder = open_fixture(&fixture);
    decoder.request(session(1), ns(0)).unwrap();
    let pid = wait_probe_pid(&marker);
    // Allow the pipe reader to reach EOF; cancellation must still interrupt
    // the pending child wait, rather than only the completed stdout read.
    std::thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    decoder.shutdown();
    decoder.join().unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "shutdown waited for sleeping probe"
    );
    assert!(
        !PathBuf::from(format!("/proc/{pid}")).exists(),
        "probe must be reaped"
    );
}

// No fixture binary is committed. These commands are executed only when the
// coordinator explicitly runs the ignored codec tier with both tool overrides.
struct Fixture {
    directory: PathBuf,
    movie: PathBuf,
    ffmpeg: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn tool(variable: &str) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!("actual codec tier requires explicit {variable}; environment incomplete")
        })
}
fn success(command: &mut Command) -> String {
    let output = command
        .output()
        .expect("execute explicitly selected external tool");
    assert!(
        output.status.success(),
        "tool failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("tool output UTF-8")
}
fn fixture() -> Fixture {
    let ffmpeg = tool("BEATKERNEL_TEST_FFMPEG");
    let ffprobe = tool("BEATKERNEL_TEST_FFPROBE");
    let directory = std::env::temp_dir().join(format!(
        "beatkernel-video-fixture-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir(&directory).expect("create isolated codec fixture directory");
    let movie = directory.join("vfr-bframes-origin.mkv");
    let fixture = Fixture {
        directory,
        movie,
        ffmpeg,
    };
    let raw = fixture.directory.join("colors.rgba");
    let mut pixels = Vec::new();
    for color in [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
        [255, 0, 255, 255],
        [0, 255, 255, 255],
        [255, 255, 255, 255],
        [96, 96, 96, 255],
        [255, 0, 0, 255],
        [0, 255, 0, 255],
    ] {
        for _ in 0..256 {
            pixels.extend_from_slice(&color);
        }
    }
    std::fs::write(&raw, pixels).unwrap();
    let timestamps = "settb=1/1000,setpts=5000+if(eq(N\\,0)\\,0\\,if(eq(N\\,1)\\,40\\,if(eq(N\\,2)\\,140\\,if(eq(N\\,3)\\,180\\,if(eq(N\\,4)\\,400\\,if(eq(N\\,5)\\,440\\,if(eq(N\\,6)\\,600\\,if(eq(N\\,7)\\,760\\,if(eq(N\\,8)\\,1100\\,1600)))))))))";
    success(
        Command::new(&fixture.ffmpeg)
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "rawvideo",
                "-pixel_format",
                "rgba",
                "-video_size",
                "16x16",
                "-framerate",
                "25",
                "-i",
            ])
            .arg(&raw)
            .args([
                "-vf",
                timestamps,
                "-fps_mode",
                "passthrough",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv444p",
                "-enc_time_base",
                "1/1000",
                "-crf",
                "12",
                "-bf",
                "2",
                "-g",
                "4",
                "-x264-params",
                "b-adapt=0:scenecut=0",
                "-threads",
                "1",
            ])
            .arg(&fixture.movie),
    );
    let streams = success(
        Command::new(&ffprobe)
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=time_base,start_time,has_b_frames",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .arg(&fixture.movie),
    );
    assert!(
        streams.lines().any(|line| line == "has_b_frames=2"),
        "fixture needs B-frame reorder: {streams}"
    );
    assert!(
        streams.lines().any(|line| line == "start_time=5.000000"),
        "fixture needs nonzero origin: {streams}"
    );
    let base = streams
        .lines()
        .find_map(|line| line.strip_prefix("time_base="))
        .unwrap();
    let (numerator, denominator) = base.split_once('/').unwrap();
    let base =
        VideoTimeBase::new(numerator.parse().unwrap(), denominator.parse().unwrap()).unwrap();
    let points = success(
        Command::new(&ffprobe)
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "frame=best_effort_timestamp",
                "-of",
                "csv=p=0",
            ])
            .arg(&fixture.movie),
    );
    let pts: Vec<i64> = points
        .lines()
        .filter_map(|line| line.split(',').next()?.parse().ok())
        .collect();
    assert_eq!(pts.len(), 10, "actual encoded fixture frame count");
    let relative: Vec<_> = pts
        .iter()
        .map(|point| base.timestamp(*point, pts[0]).unwrap().as_nanos())
        .collect();
    assert_eq!(
        relative,
        [
            0,
            40_000_000,
            140_000_000,
            180_000_000,
            400_000_000,
            440_000_000,
            600_000_000,
            760_000_000,
            1_100_000_000,
            1_600_000_000
        ]
    );
    let keyframes = success(
        Command::new(&ffprobe)
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-skip_frame",
                "nokey",
                "-show_entries",
                "frame=best_effort_timestamp",
                "-of",
                "csv=p=0",
            ])
            .arg(&fixture.movie),
    );
    let anchors: Vec<_> = keyframes
        .lines()
        .filter_map(|line| line.split(',').next()?.parse::<i64>().ok())
        .map(|point| base.timestamp(point, pts[0]).unwrap().as_nanos())
        .collect();
    assert!(
        anchors.contains(&400_000_000),
        "fixture needs actual 400ms RAP: {anchors:?}"
    );
    fixture
}
fn open_fixture(fixture: &Fixture) -> FfmpegDecoder {
    FfmpegDecoder::open(
        VideoResourceDescriptor {
            data: VideoResource::File(fixture.movie.clone()),
            encoded_bytes: std::fs::metadata(&fixture.movie).unwrap().len(),
        },
        transform(),
        FfmpegDecoderConfig {
            executable: Some(fixture.ffmpeg.clone()),
            asset_limits: VideoAssetLimits::default(),
            max_working_bytes: 4096,
            frame_limits: VideoFrameLimits {
                max_frames: 3,
                max_bytes: 3072,
                max_frame_bytes: 1024,
            },
        },
    )
    .unwrap()
}
fn drain_to(
    decoder: &mut FfmpegDecoder,
    queue: &mut VideoFrameQueue,
    key: VideoSessionKey,
    target: Timestamp,
    need_eof: bool,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        assert!(
            Instant::now() < deadline,
            "actual decoder did not fulfill target/EOF before deadline"
        );
        let Some(event) = decoder.try_next() else {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        };
        match event {
            VideoDecodeEvent::Frame(frame) => {
                assert_eq!(
                    frame.session, key,
                    "retired generation leaked through the port"
                );
                assert!(matches!(
                    queue.push(frame).unwrap(),
                    VideoFrameAdmission::Accepted
                ));
            }
            VideoDecodeEvent::Watermark { session, through } => {
                assert_eq!(session, key);
                queue.watermark(key, through).unwrap();
                let ready = queue.select(target).is_some();
                if !need_eof && ready && through >= target {
                    return;
                }
            }
            VideoDecodeEvent::End { session, end } => {
                assert_eq!(session, key);
                queue.finish(end).unwrap();
                return;
            }
            VideoDecodeEvent::Failed { reason, .. } => panic!("actual decode failed: {reason}"),
        }
    }
}
fn color(queue: &mut VideoFrameQueue, target: i64, expected_pts: i64, expected: [u8; 3]) {
    let frame = queue.select(ns(target)).unwrap_or_else(|| {
        panic!("latest completed nonfuture actual movie frame at target {target}ns")
    });
    assert_eq!(frame.pts, ns(expected_pts));
    let pixel = &frame.image.pixels()[0..4];
    for index in 0..3 {
        assert!(
            (i16::from(pixel[index]) - i16::from(expected[index])).abs() < 12,
            "encoded frame color mismatch: {pixel:?}, expected {expected:?}"
        );
    }
    assert_eq!(pixel[3], 255);
}

#[test]
#[ignore = "actual codec tier needs explicit BEATKERNEL_TEST_FFMPEG and BEATKERNEL_TEST_FFPROBE, libx264, and root execution after all writers STOP"]
fn actual_vfr_bframes_nonzero_origin_preroll_backseek_eof_and_transform() {
    let fixture = fixture();
    let mut decoder = open_fixture(&fixture);
    assert!(decoder.capabilities().available);
    let limits = VideoFrameLimits {
        max_frames: 3,
        max_bytes: 3072,
        max_frame_bytes: 1024,
    };
    let old = session(1);
    let mut queue = VideoFrameQueue::new(old, limits).unwrap();
    decoder.request(old, ns(300_000_000)).unwrap();
    drain_to(&mut decoder, &mut queue, old, ns(300_000_000), false);
    color(&mut queue, 300_000_000, 180_000_000, [255, 255, 0]);
    color(&mut queue, 300_000_000, 180_000_000, [255, 255, 0]); // Pause repeats demand.
    decoder.retire(old);
    let seek = session(2);
    queue.reset(seek);
    decoder.request(seek, ns(650_000_000)).unwrap();
    drain_to(&mut decoder, &mut queue, seek, ns(650_000_000), false);
    color(&mut queue, 650_000_000, 600_000_000, [255, 255, 255]);
    decoder.retire(seek);
    let current = session(3);
    queue.reset(current);
    decoder.request(current, ns(130_000_000)).unwrap();
    drain_to(&mut decoder, &mut queue, current, ns(130_000_000), false);
    color(&mut queue, 130_000_000, 40_000_000, [0, 255, 0]);
    decoder.request(current, ns(2_000_000_000)).unwrap();
    drain_to(&mut decoder, &mut queue, current, ns(2_000_000_000), true);
    color(&mut queue, 2_000_000_000, 1_600_000_000, [0, 255, 0]);
    color(&mut queue, i64::MAX, 1_600_000_000, [0, 255, 0]);
    let source = Arc::clone(&queue.select(ns(i64::MAX)).unwrap().image);
    let transformed = VideoTransform {
        canvas: Some([8, 8]),
        ..transform()
    }
    .apply(&source, VideoAssetLimits::default())
    .unwrap();
    assert_eq!((transformed.raw.width(), transformed.raw.height()), (8, 8));
    assert!(queue.buffered_len() <= 3 && queue.buffered_bytes() <= 3072);
    decoder.shutdown();
    decoder.join().unwrap();
}

#[test]
#[ignore = "actual codec cancellation tier needs explicit FFmpeg/FFprobe overrides and root execution after all writers STOP"]
fn actual_decoder_cancel_joins_when_consumer_does_not_drain_bounded_output() {
    let fixture = fixture();
    let mut decoder = open_fixture(&fixture);
    decoder.request(session(1), ns(2_000_000_000)).unwrap();
    std::thread::sleep(Duration::from_millis(100)); // Deliberately withhold consumer credits.
    let begin = Instant::now();
    decoder.retire(session(1));
    assert!(
        decoder.try_next().is_none(),
        "retirement must fence queued pixels immediately"
    );
    decoder.shutdown();
    assert!(
        begin.elapsed() < Duration::from_secs(1),
        "callback cancellation must not join child IO"
    );
    decoder.join().unwrap();
    assert!(
        begin.elapsed() < Duration::from_secs(10),
        "pipe tasks and child must reap without consumer drain"
    );
    assert!(decoder.try_next().is_none());
}
