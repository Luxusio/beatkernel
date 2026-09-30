//! Checked recorded BMS sounds on the explicit host output backend; no input.
use beatkernel::{
    audio::{command_queue, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, RenderReport},
    input::CodecLimits,
    replay::codec::ReplayCodecLimits,
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms_runtime::{
    bgm::{BgmConfig, BgmFeeder},
    load_prepared,
    replay_audio::{completed_render_cursor, plan_audio},
    replay_playback::read_replay,
    ChannelPolicy,
};
use beatkernel_platform::audio::{DeviceFormat, SampleEncoding, SharedPeriodPolicy};
use std::{
    collections::HashSet,
    error::Error,
    fs::File,
    path::PathBuf,
    time::{Duration as WallDuration, Instant},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const OUTPUT: ClockDomainId = ClockDomainId(0x4252504c);
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
const HOST: ClockDomainId = ClockDomainId(0x42525048);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backend {
    Windows,
    Linux,
    Macos,
    Unsupported,
}
fn host_backend() -> Backend {
    if cfg!(target_os = "windows") {
        Backend::Windows
    } else if cfg!(target_os = "linux") {
        Backend::Linux
    } else if cfg!(target_os = "macos") {
        Backend::Macos
    } else {
        Backend::Unsupported
    }
}
#[derive(Debug)]
struct Options {
    chart: PathBuf,
    replay: PathBuf,
    device: String,
    seconds: u64,
    format: AudioFormat,
    buffer: Option<u32>,
    #[cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]
    period: Option<u32>,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    exclusive: bool,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    shared: SharedPeriodPolicy,
    preroll: Duration,
    lookahead: Duration,
    capacity: usize,
    voices: usize,
    max_records: usize,
    max_bytes: usize,
}
fn positive_usize(value: &str) -> Result<usize> {
    let count: usize = value.parse()?;
    if count == 0 {
        return Err("capacity must be a positive usize".into());
    }
    Ok(count)
}
fn positive_u32(value: &str) -> Result<u32> {
    let count: u32 = value.parse()?;
    if count == 0 {
        return Err("frame count must be a positive u32".into());
    }
    Ok(count)
}
fn parse(args: &[String], backend: Backend) -> Result<Options> {
    let (mut chart, mut replay, mut device, mut seconds, mut rate, mut channels) =
        (None, None, None, None, None, None);
    let (mut buffer, mut period) = (None, None);
    let mut exclusive = false;
    let mut shared = SharedPeriodPolicy::EnginePeriod;
    let mut mode_set = false;
    let mut shared_set = false;
    let mut preroll = 3_000_000_000i64;
    let mut lookahead = 3_000_000_000i64;
    let mut capacity = 65536usize;
    let mut voices = 4096usize;
    let mut max_records = 1_000_000usize;
    let mut max_bytes = 64 * 1024 * 1024usize;
    let mut seen = HashSet::new();
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        if !seen.insert(flag.as_str()) {
            return Err(format!("duplicate option {flag}").into());
        }
        let value = args.next().ok_or("each option requires a value")?;
        match flag.as_str() {
            "--chart" | "--replay" => {
                if value.is_empty() {
                    return Err(format!("{flag} requires a nonempty path").into());
                }
                if flag == "--chart" {
                    chart = Some(PathBuf::from(value));
                } else {
                    replay = Some(PathBuf::from(value));
                }
            }
            "--device" => {
                if value.is_empty() || value.contains('\0') {
                    return Err("device ID must be nonempty with no NUL".into());
                }
                device = Some(value.clone());
            }
            "--seconds" => {
                let count: u64 = value.parse()?;
                if count == 0 {
                    return Err("seconds must be positive u64".into());
                }
                Instant::now()
                    .checked_add(WallDuration::from_secs(count))
                    .ok_or("wall duration is not representable")?;
                seconds = Some(count);
            }
            "--rate" => rate = Some(value.parse::<u32>()?),
            "--channels" => channels = Some(value.parse::<u16>()?),
            "--buffer-frames" => buffer = Some(positive_u32(value)?),
            "--period-frames" => period = Some(positive_u32(value)?),
            "--mode" => {
                mode_set = true;
                exclusive = match value.as_str() {
                    "shared" => false,
                    "exclusive" => true,
                    _ => return Err("mode must be shared or exclusive".into()),
                };
            }
            "--shared-policy" => {
                shared_set = true;
                shared = match value.as_str() {
                    "engine-period" => SharedPeriodPolicy::EnginePeriod,
                    "legacy" => SharedPeriodPolicy::DeviceDefault,
                    _ => return Err("shared policy must be engine-period or legacy".into()),
                };
            }
            "--preroll-ns" => {
                preroll = value.parse()?;
                if preroll < 0 {
                    return Err("preroll must be nonnegative i64 nanoseconds".into());
                }
            }
            "--lookahead-ns" => {
                lookahead = value.parse()?;
                if lookahead <= 0 {
                    return Err("lookahead must be positive i64 nanoseconds".into());
                }
            }
            "--command-capacity" => capacity = positive_usize(value)?,
            "--voices" => voices = positive_usize(value)?,
            "--max-records" => max_records = positive_usize(value)?,
            "--max-bytes" => max_bytes = positive_usize(value)?,
            _ => return Err(format!("unknown option {flag}").into()),
        }
    }
    let device = device.ok_or("--device ID is required")?;
    if backend != Backend::Windows && (mode_set || shared_set) {
        return Err("--mode and --shared-policy apply only to Windows".into());
    }
    if exclusive && shared_set {
        return Err("explicit --shared-policy cannot accompany exclusive mode".into());
    }
    match backend {
        Backend::Linux => {
            let period = period.ok_or("ALSA requires explicit --period-frames N")?;
            let buffer = buffer.ok_or("ALSA requires explicit --buffer-frames N")?;
            if period >= buffer || period as usize > AudioLimits::MAX_RENDER_FRAMES {
                return Err(
                    "ALSA requires period < buffer and period within the core render ceiling"
                        .into(),
                );
            }
        }
        Backend::Macos => {
            let id: u32 = device.parse()?;
            if id == 0 {
                return Err("CoreAudio requires a positive numeric AudioDeviceID".into());
            }
            if period.is_some() {
                return Err("--period-frames does not apply to CoreAudio".into());
            }
            let buffer = buffer.ok_or("CoreAudio requires explicit --buffer-frames N")?;
            if buffer as usize > AudioLimits::MAX_RENDER_FRAMES {
                return Err("CoreAudio buffer exceeds core render ceiling".into());
            }
        }
        Backend::Windows => {
            if buffer.is_some_and(|frames| frames as usize > AudioLimits::MAX_RENDER_FRAMES)
                || period.is_some_and(|frames| frames as usize > AudioLimits::MAX_RENDER_FRAMES)
            {
                return Err("WASAPI requested sizes exceed core render ceiling".into());
            }
        }
        Backend::Unsupported => {
            return Err("native replay output supports only Windows, Linux and macOS".into())
        }
    }
    let format = AudioFormat::new(
        rate.ok_or("--rate HZ is required")?,
        channels.ok_or("--channels N is required")?,
    )?;
    DeviceFormat::new(
        format.sample_rate(),
        format.channels(),
        SampleEncoding::Float32,
        None,
    )?;
    AudioLimits::new(
        capacity,
        voices,
        capacity,
        AudioLimits::MAX_RENDER_FRAMES,
        capacity,
    )?;
    ReplayCodecLimits::new(
        max_bytes,
        max_records,
        4096,
        CodecLimits::new(65536, 32768)?,
    )?;
    Ok(Options {
        chart: chart.ok_or("--chart PATH is required")?,
        replay: replay.ok_or("--replay PATH is required")?,
        device,
        seconds: seconds.ok_or("--seconds N is required")?,
        format,
        buffer,
        period,
        exclusive,
        shared,
        preroll: Duration::from_nanos(preroll),
        lookahead: Duration::from_nanos(lookahead),
        capacity,
        voices,
        max_records,
        max_bytes,
    })
}

trait NativeOutput {
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn poll(&self) -> Result<Option<RenderReport>>;
    fn last_render(&self) -> Option<RenderReport>;
    fn final_check(&self) -> Result<()>;
    fn print_native(&self);
}

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use beatkernel_platform::{
        audio::{
            AudioBackendKind, AudioDeviceId, AudioOutputStream, AudioStreamMode,
            AudioStreamRequest, AudioStreamStatus, BufferRequest, PeriodRequest,
        },
        windows::{
            audio::{WasapiBackend, WasapiOptions, WasapiStream},
            clock::QpcClock,
        },
    };
    struct Stream(WasapiStream);
    impl Stream {
        fn check(
            snapshot: beatkernel_platform::audio::AudioStreamSnapshot,
            final_check: bool,
        ) -> Result<()> {
            match snapshot.status {
                AudioStreamStatus::Running => {}
                AudioStreamStatus::Stopped if final_check => {}
                status => {
                    return Err(format!("WASAPI unexpected/terminal status: {status:?}").into())
                }
            }
            if snapshot.telemetry_available && snapshot.counters.native_failures != 0 {
                return Err(format!("WASAPI native failures: {snapshot:?}").into());
            }
            Ok(())
        }
    }
    impl NativeOutput for Stream {
        fn start(&mut self) -> Result<()> {
            Ok(self.0.start()?)
        }
        fn stop(&mut self) -> Result<()> {
            Ok(self.0.stop()?)
        }
        fn poll(&self) -> Result<Option<RenderReport>> {
            let snapshot = self.0.snapshot();
            Self::check(snapshot, false)?;
            Ok(snapshot.render)
        }
        fn last_render(&self) -> Option<RenderReport> {
            self.0.snapshot().render
        }
        fn final_check(&self) -> Result<()> {
            Self::check(self.0.snapshot(), true)
        }
        fn print_native(&self) {
            println!("WASAPI applied={:?}; native snapshot={:?}; inferred counters are not acoustic proof", self.0.configuration(), self.0.snapshot());
        }
    }
    pub(super) fn open(options: &Options, mixer: Mixer) -> Result<Box<dyn NativeOutput>> {
        let request = AudioStreamRequest::new(
            AudioDeviceId(options.device.clone()),
            AudioBackendKind::Wasapi,
            if options.exclusive {
                AudioStreamMode::Exclusive
            } else {
                AudioStreamMode::Shared(options.shared)
            },
            DeviceFormat::new(
                options.format.sample_rate(),
                options.format.channels(),
                SampleEncoding::Float32,
                None,
            )?,
            options
                .buffer
                .map_or(BufferRequest::DeviceDefault, BufferRequest::Frames),
            options
                .period
                .map_or(PeriodRequest::DeviceDefault, PeriodRequest::Frames),
        )?;
        let stream = WasapiBackend.open(
            request,
            mixer,
            QpcClock::new(HOST)?,
            WasapiOptions::default(),
        )?;
        println!(
            "WASAPI requested/applied exact float32 output={:?}",
            stream.configuration()
        );
        Ok(Box::new(Stream(stream)))
    }
}

#[cfg(target_os = "linux")]
mod native {
    use super::*;
    use beatkernel_platform::linux::{AlsaRequest, AlsaStatus, AlsaStream};
    struct Stream(AlsaStream);
    impl Stream {
        fn check(&self, final_check: bool) -> Result<()> {
            let snapshot = self.0.snapshot();
            match snapshot.status {
                AlsaStatus::Running => {}
                AlsaStatus::Ready if !final_check => return Ok(()), // asynchronous start flag
                AlsaStatus::Stopped if final_check => {}
                status => return Err(format!("ALSA unexpected/terminal status: {status:?}").into()),
            }
            if snapshot.failures != 0 || snapshot.xruns != 0 || snapshot.suspends != 0 {
                return Err(format!("ALSA actual native error counters: {snapshot:?}").into());
            }
            Ok(())
        }
    }
    impl NativeOutput for Stream {
        fn start(&mut self) -> Result<()> {
            Ok(self.0.start()?)
        }
        fn stop(&mut self) -> Result<()> {
            Ok(self.0.stop()?)
        }
        fn poll(&self) -> Result<Option<RenderReport>> {
            self.check(false)?;
            Ok(self.0.last_render_report())
        }
        fn last_render(&self) -> Option<RenderReport> {
            self.0.last_render_report()
        }
        fn final_check(&self) -> Result<()> {
            self.check(true)
        }
        fn print_native(&self) {
            println!("ALSA applied={:?}; independent native counters={:?}; retained core report does not prove native writes/acoustic output", self.0.configuration(), self.0.snapshot());
        }
    }
    pub(super) fn open(options: &Options, mixer: Mixer) -> Result<Box<dyn NativeOutput>> {
        let request = AlsaRequest {
            device: options.device.clone(),
            format: DeviceFormat::new(
                options.format.sample_rate(),
                options.format.channels(),
                SampleEncoding::Float32,
                None,
            )?,
            buffer_frames: options.buffer.ok_or("ALSA buffer required")?,
            period_frames: options.period.ok_or("ALSA period required")?,
            allow_size_rounding: false,
            monotonic_domain: HOST,
        };
        let stream = AlsaStream::open(request, mixer)?;
        println!(
            "ALSA requested/applied exact float32 output={:?}",
            stream.configuration()
        );
        Ok(Box::new(Stream(stream)))
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use beatkernel_platform::macos::{
        audio::{CoreAudioRequest, CoreAudioStream},
        clock::MachClock,
    };
    struct Stream(CoreAudioStream);
    impl Stream {
        fn check(&self) -> Result<()> {
            let snapshot = self.0.snapshot();
            if snapshot.configuration_changed || snapshot.callback_failures != 0 {
                return Err(
                    format!("CoreAudio configuration/callback failure: {snapshot:?}").into(),
                );
            }
            Ok(())
        }
    }
    impl NativeOutput for Stream {
        fn start(&mut self) -> Result<()> {
            Ok(self.0.start()?)
        }
        fn stop(&mut self) -> Result<()> {
            Ok(self.0.stop()?)
        }
        fn poll(&self) -> Result<Option<RenderReport>> {
            self.check()?;
            Ok(self.0.last_render_report())
        }
        fn last_render(&self) -> Option<RenderReport> {
            self.0.last_render_report()
        }
        fn final_check(&self) -> Result<()> {
            self.check()
        }
        fn print_native(&self) {
            println!("CoreAudio applied={:?}; native counters={:?}; retained render is distinct from callback delivery/acoustic output", self.0.configuration(), self.0.snapshot());
        }
    }
    pub(super) fn open(options: &Options, mixer: Mixer) -> Result<Box<dyn NativeOutput>> {
        let request = CoreAudioRequest {
            device: options.device.parse()?,
            format: options.format,
            buffer_frames: options.buffer.ok_or("CoreAudio buffer required")?,
        };
        let clock = MachClock::new(ClockDomainId(0x4252504d), HOST)?;
        let stream = CoreAudioStream::open(request, clock, mixer)?;
        println!(
            "CoreAudio requested/applied exact float32 output={:?}",
            stream.configuration()
        );
        Ok(Box::new(Stream(stream)))
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod native {
    use super::*;
    pub(super) fn open(_: &Options, _: Mixer) -> Result<Box<dyn NativeOutput>> {
        Err("native replay output requires Windows, Linux or macOS".into())
    }
}

fn run(options: Options) -> Result<()> {
    let limits = ReplayCodecLimits::new(
        options.max_bytes,
        options.max_records,
        4096,
        CodecLimits::new(65536, 32768)?,
    )?;
    let file = read_replay(&mut File::open(&options.replay)?, limits)?;
    let prepared = load_prepared(
        &options.chart,
        options.format,
        PcmLimits::new(64 * 1024 * 1024, 256 * 1024 * 1024, 1295)?,
        ChannelPolicy::Exact,
    )?;
    for warning in &prepared.source.warnings {
        eprintln!("BMS warning line {}: {}", warning.line, warning.message);
    }
    let origin = ClockPoint {
        domain: OUTPUT,
        timestamp: Timestamp::ZERO,
    };
    let plan = plan_audio(&prepared, file, limits, origin, options.preroll)?;
    println!("reconstructed logical replay: results={} hits={} recorded_until={:?} final_judge_hash={:#018x}; no live acquisition or original physical timing reproduction", plan.judge_events.len(), plan.judge_events.iter().filter(|event| matches!(event.outcome, beatkernel::judge::JudgeOutcome::Hit { .. })).count(), plan.recorded_until, plan.final_judge_hash);
    let mut feeder = BgmFeeder::from_output_commands(
        plan.commands,
        BgmConfig {
            output_origin: origin,
            sample_rate: options.format.sample_rate(),
            preroll: Duration::ZERO,
            lookahead: options.lookahead,
            max_pending: options.capacity,
        },
    )?;
    let (mut producer, consumer) = command_queue(options.capacity)?;
    if let Err(error) = feeder.feed(0, options.capacity, |command| producer.try_push(command)) {
        eprintln!(
            "initial admission failed: {error}; config={:?}; valid admitted prefix={:?}",
            feeder.config(),
            feeder.report()
        );
        return Err(error.into());
    }
    let mixer = Mixer::new(
        MixerConfig::new(
            options.format,
            origin.domain,
            origin.timestamp,
            AudioLimits::new(
                options.capacity,
                options.voices,
                options.capacity,
                AudioLimits::MAX_RENDER_FRAMES,
                options.capacity,
            )?,
        ),
        prepared.bank,
        consumer,
    )?;
    let mut stream = match native::open(&options, mixer) {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("native open failed: {error}; feeder config={:?}; admitted prefix={:?}; no native output claimed", feeder.config(), feeder.report());
            return Err(error);
        }
    };
    let outcome = (|| -> Result<()> {
        stream.start()?;
        let deadline = Instant::now()
            .checked_add(WallDuration::from_secs(options.seconds))
            .ok_or("wall duration is not representable")?;
        while Instant::now() < deadline {
            if let Some(report) = stream.poll()? {
                let cursor = completed_render_cursor(&report)?;
                feeder.feed(cursor, 256, |command| producer.try_push(command))?;
            }
            std::thread::sleep(WallDuration::from_millis(1));
        }
        Ok(())
    })();
    let stop = stream.stop();
    let final_native = stream.final_check();
    let report = stream.last_render();
    let final_core = report.as_ref().map(completed_render_cursor).transpose();
    println!("final command admission config={:?}; summary={:?}; admission is separate from core execution/native delivery/acoustic output", feeder.config(), feeder.report());
    match report {
        Some(report) => println!("last completed typed core RenderReport={report:?}"),
        None => println!("last completed core RenderReport unavailable; no zero cursor fabricated"),
    }
    stream.print_native();
    if let Err(error) = &outcome {
        eprintln!("native replay operation failed: {error}");
    }
    if let Err(error) = &stop {
        eprintln!("native stop/close failed: {error}");
    }
    if let Err(error) = &final_native {
        eprintln!("final native failure: {error}");
    }
    if let Err(error) = &final_core {
        eprintln!("final core execution failure: {error}");
    }
    outcome?;
    stop?;
    final_native?;
    final_core?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] {
        println!("play_replay_bms --chart PATH --replay PATH --device ID --seconds N --rate HZ --channels N [--buffer-frames N --period-frames N --mode shared|exclusive --shared-policy engine-period|legacy --preroll-ns N --lookahead-ns N --command-capacity N --voices N --max-records N --max-bytes N]\nHost backend: WASAPI on Windows, ALSA on Linux, CoreAudio on macOS. No input acquisition. Exact float32 rate/channels; no endpoint/mode fallback.\nLinux requires explicit buffer/period; macOS requires numeric AudioDeviceID and buffer, rejects period. Mode/shared-policy are Windows only, default shared engine-period; explicit shared-policy rejects exclusive. Windows buffer/period default to device settings, and unsupported requested combinations reject.\nDefaults: preroll 3000000000ns, lookahead 3000000000ns, commands 65536, voices 4096, records 1000000, replay bytes 67108864. Nonnegative i64 preroll, positive i64 lookahead, positive checked finite seconds and capacities.\nSeconds is wall playback duration after Start including preroll; no automatic tail drain. Finite horizons/credit can fail on stalls/dense cues; final admission/core/native diagnostics remain separate. Source implementation is not native sound or physical timing evidence.");
        return Ok(());
    }
    run(parse(&args, host_backend())?)
}

#[cfg(test)]
mod fixtures {
    use super::*;
    fn args(extra: &[&str]) -> Vec<String> {
        [
            "--chart",
            "chart.bms",
            "--replay",
            "session.bkr",
            "--device",
            "42",
            "--seconds",
            "2",
            "--rate",
            "48000",
            "--channels",
            "2",
        ]
        .into_iter()
        .chain(extra.iter().copied())
        .map(str::to_owned)
        .collect()
    }
    #[test]
    fn explicit_host_settings_and_backend_option_rejection() {
        let windows = parse(&args(&[]), Backend::Windows).unwrap();
        assert!(!windows.exclusive);
        assert_eq!(windows.shared, SharedPeriodPolicy::EnginePeriod);
        assert_eq!(windows.preroll.as_nanos(), 3_000_000_000);
        assert_eq!(windows.lookahead.as_nanos(), 3_000_000_000);
        assert_eq!(windows.capacity, 65536);
        assert_eq!(windows.voices, 4096);
        assert_eq!(windows.max_records, 1_000_000);
        assert_eq!(windows.max_bytes, 64 * 1024 * 1024);
        assert_eq!(
            parse(&args(&["--shared-policy", "legacy"]), Backend::Windows)
                .unwrap()
                .shared,
            SharedPeriodPolicy::DeviceDefault
        );
        assert!(parse(
            &args(&["--mode", "exclusive", "--shared-policy", "engine-period"]),
            Backend::Windows
        )
        .is_err());
        assert!(parse(&args(&[]), Backend::Linux).is_err());
        assert!(parse(
            &args(&["--buffer-frames", "256", "--period-frames", "64"]),
            Backend::Linux
        )
        .is_ok());
        assert!(parse(
            &args(&["--buffer-frames", "64", "--period-frames", "64"]),
            Backend::Linux
        )
        .is_err());
        assert!(parse(
            &args(&[
                "--buffer-frames",
                "256",
                "--period-frames",
                "64",
                "--mode",
                "shared"
            ]),
            Backend::Linux
        )
        .is_err());
        assert!(parse(
            &args(&["--buffer-frames", "256", "--shared-policy", "legacy"]),
            Backend::Macos
        )
        .is_err());
        assert!(parse(&args(&["--buffer-frames", "256"]), Backend::Macos).is_ok());
        assert!(parse(
            &args(&["--buffer-frames", "256", "--period-frames", "64"]),
            Backend::Macos
        )
        .is_err());
        assert!(parse(&args(&[]), Backend::Unsupported).is_err());
    }
    #[test]
    fn strict_paths_duplicates_and_checked_integer_bounds() {
        assert!(parse(&[], Backend::Windows).is_err());
        assert!(parse(&args(&["--unknown", "x"]), Backend::Windows).is_err());
        assert!(parse(&args(&["--voices"]), Backend::Windows).is_err());
        for flag in [
            "--chart",
            "--replay",
            "--device",
            "--seconds",
            "--rate",
            "--channels",
        ] {
            assert!(parse(&args(&[flag, "1"]), Backend::Windows).is_err());
        }
        for flag in [
            "--preroll-ns",
            "--lookahead-ns",
            "--command-capacity",
            "--voices",
            "--max-records",
            "--max-bytes",
            "--buffer-frames",
            "--period-frames",
            "--mode",
            "--shared-policy",
        ] {
            let value = match flag {
                "--mode" => "shared",
                "--shared-policy" => "legacy",
                _ => "1",
            };
            assert!(parse(&args(&[flag, value, flag, value]), Backend::Windows).is_err());
        }
        for flag in [
            "--command-capacity",
            "--voices",
            "--max-records",
            "--max-bytes",
            "--buffer-frames",
            "--period-frames",
            "--lookahead-ns",
        ] {
            for value in ["0", "-1", "184467440737095516160"] {
                assert!(parse(&args(&[flag, value]), Backend::Windows).is_err());
            }
        }
        assert!(parse(&args(&["--preroll-ns", "-1"]), Backend::Windows).is_err());
        assert!(parse(&args(&["--command-capacity", "65537"]), Backend::Windows).is_err());
        assert!(parse(&args(&["--voices", "4097"]), Backend::Windows).is_err());
        for flag in ["--chart", "--replay", "--device"] {
            let mut invalid = args(&[]);
            let index = invalid.iter().position(|value| value == flag).unwrap();
            invalid[index + 1].clear();
            assert!(parse(&invalid, Backend::Windows).is_err());
        }
        for (flag, value) in [
            ("--seconds", "0"),
            ("--seconds", "18446744073709551615"),
            ("--rate", "0"),
            ("--channels", "0"),
            ("--channels", "33"),
        ] {
            let mut invalid = args(&[]);
            let index = invalid.iter().position(|value| value == flag).unwrap();
            invalid[index + 1] = value.into();
            assert!(parse(&invalid, Backend::Windows).is_err());
        }
    }
}
