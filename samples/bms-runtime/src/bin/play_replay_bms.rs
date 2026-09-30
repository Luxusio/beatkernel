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
    Asio,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AsioView {
    Native,
    Bits32,
    Bits64,
}
#[derive(Debug)]
struct Options {
    backend: Backend,
    #[cfg_attr(
        not(all(target_os = "windows", feature = "asio-sdk")),
        allow(dead_code)
    )]
    asio_view: Option<AsioView>,
    #[cfg_attr(
        not(all(target_os = "windows", feature = "asio-sdk")),
        allow(dead_code)
    )]
    output_channels: Option<Vec<u32>>,
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
fn parse(args: &[String], host: Backend) -> Result<Options> {
    let mut backend = host;
    let mut asio_view = None;
    let mut output_channels = None;
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
            "--backend" => {
                backend = match value.as_str() {
                    "wasapi" => Backend::Windows,
                    "asio" => Backend::Asio,
                    "alsa" => Backend::Linux,
                    "coreaudio" => Backend::Macos,
                    _ => return Err("backend must be wasapi, asio, alsa or coreaudio".into()),
                }
            }
            "--asio-view" => {
                asio_view = Some(match value.as_str() {
                    "native" => AsioView::Native,
                    "32" => AsioView::Bits32,
                    "64" => AsioView::Bits64,
                    _ => return Err("ASIO view must be native, 32 or 64".into()),
                })
            }
            "--output-channels" => {
                let mut selected = Vec::new();
                for token in value.split(',') {
                    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
                        return Err("ASIO channel tokens must be unsigned decimal indices".into());
                    }
                    let index: u32 = token.parse()?;
                    if index > i32::MAX as u32 || selected.contains(&index) || selected.len() == 32
                    {
                        return Err("invalid or duplicate ASIO output channel".into());
                    }
                    selected.push(index);
                }
                output_channels = Some(selected);
            }

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
    let mut device = device.ok_or("--device ID is required")?;
    if backend == Backend::Asio && host != Backend::Windows {
        return Err("ASIO requires a Windows host".into());
    }
    if backend != host && !(host == Backend::Windows && backend == Backend::Asio) {
        return Err("requested backend is incompatible with host".into());
    }
    if backend != Backend::Asio && (asio_view.is_some() || output_channels.is_some()) {
        return Err("ASIO flags require --backend asio".into());
    }
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
        Backend::Asio => {
            if period.is_some() || mode_set || shared_set {
                return Err("ASIO rejects period/mode/shared-policy flags".into());
            }
            if asio_view.is_none() || output_channels.is_none() {
                return Err("ASIO requires --asio-view and --output-channels".into());
            }
            if buffer.is_some_and(|n| n as usize > AudioLimits::MAX_RENDER_FRAMES) {
                return Err("ASIO buffer exceeds core render ceiling".into());
            }
            let bytes = device.as_bytes();
            if bytes.len() != 38
                || bytes[0] != b'{'
                || bytes[37] != b'}'
                || bytes[1..37].iter().enumerate().any(|(i, b)| {
                    if [8, 13, 18, 23].contains(&i) {
                        *b != b'-'
                    } else {
                        !b.is_ascii_hexdigit()
                    }
                })
                || !bytes[1..37]
                    .iter()
                    .any(|b| b.is_ascii_hexdigit() && *b != b'0')
            {
                return Err("ASIO device requires a nonzero braced UUID CLSID".into());
            }
            device.make_ascii_uppercase();
        }
        Backend::Unsupported => {
            return Err("native replay output supports only Windows, Linux and macOS".into())
        }
    }
    let format = AudioFormat::new(
        rate.ok_or("--rate HZ is required")?,
        channels.ok_or("--channels N is required")?,
    )?;
    if output_channels
        .as_ref()
        .is_some_and(|v| v.len() != usize::from(format.channels()))
    {
        return Err("ASIO output channel count must match --channels".into());
    }
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
        backend,
        asio_view,
        output_channels,
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
    fn poll(&mut self) -> Result<Option<RenderReport>>;
    fn last_render(&mut self) -> Option<RenderReport>;
    fn final_check(&mut self) -> Result<()>;
    fn print_native(&mut self);
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
        fn poll(&mut self) -> Result<Option<RenderReport>> {
            let snapshot = self.0.snapshot();
            Self::check(snapshot, false)?;
            Ok(snapshot.render)
        }
        fn last_render(&mut self) -> Option<RenderReport> {
            self.0.snapshot().render
        }
        fn final_check(&mut self) -> Result<()> {
            Self::check(self.0.snapshot(), true)
        }
        fn print_native(&mut self) {
            println!("WASAPI applied={:?}; native snapshot={:?}; inferred counters are not acoustic proof", self.0.configuration(), self.0.snapshot());
        }
    }
    pub(super) fn open(options: &Options, mixer: Mixer) -> Result<Box<dyn NativeOutput>> {
        if options.backend == Backend::Asio {
            #[cfg(feature = "asio-sdk")]
            {
                return super::asio_native::open(options, mixer);
            }
            #[cfg(not(feature = "asio-sdk"))]
            {
                return Err("ASIO requires feature asio-sdk".into());
            }
        }
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
        fn poll(&mut self) -> Result<Option<RenderReport>> {
            self.check(false)?;
            Ok(self.0.last_render_report())
        }
        fn last_render(&mut self) -> Option<RenderReport> {
            self.0.last_render_report()
        }
        fn final_check(&mut self) -> Result<()> {
            self.check(true)
        }
        fn print_native(&mut self) {
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
        fn poll(&mut self) -> Result<Option<RenderReport>> {
            self.check()?;
            Ok(self.0.last_render_report())
        }
        fn last_render(&mut self) -> Option<RenderReport> {
            self.0.last_render_report()
        }
        fn final_check(&mut self) -> Result<()> {
            self.check()
        }
        fn print_native(&mut self) {
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
    if options.backend == Backend::Asio && !cfg!(all(target_os = "windows", feature = "asio-sdk")) {
        return Err(
            "ASIO requires Windows and sample feature asio-sdk with supplied SDK/MSVC toolchain"
                .into(),
        );
    }
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
        println!("play_replay_bms --chart PATH --replay PATH --device ID --seconds N --rate HZ --channels N [--backend wasapi|asio|alsa|coreaudio --asio-view native|32|64 --output-channels 0,1 --buffer-frames N --period-frames N --mode shared|exclusive --shared-policy engine-period|legacy --preroll-ns N --lookahead-ns N --command-capacity N --voices N --max-records N --max-bytes N]\nASIO requires Windows + asio-sdk, explicit braced CLSID/view/output channels, rejects mode/shared-policy/period, buffer default driver preferred.\nHost backend: WASAPI on Windows, ALSA on Linux, CoreAudio on macOS. No input acquisition. Exact float32 rate/channels; no endpoint/mode fallback.\nLinux requires explicit buffer/period; macOS requires numeric AudioDeviceID and buffer, rejects period. Mode/shared-policy are Windows only, default shared engine-period; explicit shared-policy rejects exclusive. Windows buffer/period default to device settings, and unsupported requested combinations reject.\nDefaults: preroll 3000000000ns, lookahead 3000000000ns, commands 65536, voices 4096, records 1000000, replay bytes 67108864. Nonnegative i64 preroll, positive i64 lookahead, positive checked finite seconds and capacities.\nSeconds is wall playback duration after Start including preroll; no automatic tail drain. Finite horizons/credit can fail on stalls/dense cues; final admission/core/native diagnostics remain separate. Source implementation is not native sound or physical timing evidence.");
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

#[cfg(test)]
#[path = "play_replay_bms/asio_fixtures.rs"]
mod asio_fixtures;

#[cfg(all(target_os = "windows", feature = "asio-sdk"))]
#[allow(unsafe_code)]
mod asio_native {
    use super::*;
    use beatkernel_platform::{
        audio::asio::AsioBufferRequest,
        windows::asio::{
            control::AsioControl,
            enumerate_asio_drivers,
            stream::{AsioStream, AsioStreamPhase, AsioStreamSnapshot},
            AsioEnumerationLimits, AsioRegistryView,
        },
    };
    use std::{io, ptr};
    use windows_sys::Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
            RegisterClassW, TranslateMessage, UnregisterClassW, MSG, PM_REMOVE, WM_CLOSE, WM_QUIT,
            WNDCLASSW,
        },
    };
    unsafe extern "system" fn host_window(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // Keep the hidden system reference alive until explicit driver teardown.
        // The control loop owns stop; WM_CLOSE must not destroy a window still
        // retained by the ASIO driver.
        if message == WM_CLOSE {
            return 0;
        }
        // SAFETY: parameters originate in Windows dispatch; no Rust userdata is retained.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }
    struct Window {
        hwnd: HWND,
        instance: HINSTANCE,
        class: Vec<u16>,
    }
    impl Window {
        fn new() -> Result<Self> {
            let class: Vec<u16> = format!("BeatKernelAsioReplay{}", std::process::id())
                .encode_utf16()
                .chain(Some(0))
                .collect();
            // SAFETY: null requests the current executable module.
            let instance = unsafe { GetModuleHandleW(ptr::null()) };
            if instance.is_null() {
                return Err(io::Error::last_os_error().into());
            }
            // SAFETY: zero is valid for all unused native class fields.
            let mut descriptor: WNDCLASSW = unsafe { std::mem::zeroed() };
            descriptor.lpfnWndProc = Some(host_window);
            descriptor.hInstance = instance;
            descriptor.lpszClassName = class.as_ptr();
            // SAFETY: live terminated class name; system WNDPROC retains no Rust data.
            if unsafe { RegisterClassW(&descriptor) } == 0 {
                return Err(io::Error::last_os_error().into());
            }
            let mut window = Self {
                hwnd: ptr::null_mut(),
                instance,
                class,
            };
            // SAFETY: registered live class/module; hidden owner-thread window with no userdata.
            window.hwnd = unsafe {
                CreateWindowExW(
                    0,
                    window.class.as_ptr(),
                    window.class.as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    instance,
                    ptr::null(),
                )
            };
            if window.hwnd.is_null() {
                return Err(io::Error::last_os_error().into());
            }
            Ok(window)
        }
        fn pump(&mut self) -> Result<()> {
            // SAFETY: all-zero native message storage is valid before PeekMessage fills it.
            let mut message: MSG = unsafe { std::mem::zeroed() };
            for _ in 0..256 {
                // SAFETY: owner-thread message storage is live; bounded dispatch off audio thread.
                if unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } == 0 {
                    break;
                }
                if message.message == WM_QUIT {
                    return Err("ASIO host received WM_QUIT".into());
                }
                // SAFETY: message was returned by this thread's native queue.
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            Ok(())
        }
    }
    impl Drop for Window {
        fn drop(&mut self) {
            // SAFETY: owner thread; Stream stop/drop drains driver before this window is destroyed.
            unsafe {
                if !self.hwnd.is_null() {
                    DestroyWindow(self.hwnd);
                }
                UnregisterClassW(self.class.as_ptr(), self.instance);
            }
        }
    }
    // Declaration order also drops stream before the driver's caller-owned HWND.
    struct Stream {
        stream: AsioStream,
        window: Window,
        retained: Option<RenderReport>,
    }
    impl Stream {
        fn snapshot(&mut self, final_check: bool) -> Result<AsioStreamSnapshot> {
            let snapshot = self.stream.snapshot()?;
            if snapshot.render.is_some() {
                self.retained = snapshot.render;
            }
            if snapshot.native.faults.requires_reopen()
                || snapshot.native.render_error != 0
                || snapshot.native.faults.0 & 32 != 0
            {
                return Err(format!("ASIO callback/native fault or overload: {snapshot:?}").into());
            }
            match snapshot.phase {
                AsioStreamPhase::Running => {}
                AsioStreamPhase::Stopped if final_check => {}
                phase => return Err(format!("ASIO unexpected/terminal phase: {phase:?}").into()),
            }
            Ok(snapshot)
        }
    }
    impl NativeOutput for Stream {
        fn start(&mut self) -> Result<()> {
            Ok(self.stream.start()?)
        }
        fn stop(&mut self) -> Result<()> {
            let result = self.stream.stop();
            println!("closed ASIO render-start QPC cadence={:?}; priming excluded, native delivery/acoustic jitter unmeasured", self.stream.render_cadence());
            if let Ok(snapshot) = self.stream.snapshot() {
                if snapshot.render.is_some() {
                    self.retained = snapshot.render;
                }
            }
            Ok(result?)
        }
        fn poll(&mut self) -> Result<Option<RenderReport>> {
            self.window.pump()?;
            let snapshot = self.snapshot(false)?;
            Ok(if snapshot.telemetry_available {
                snapshot.render
            } else {
                None
            })
        }
        fn last_render(&mut self) -> Option<RenderReport> {
            if let Ok(snapshot) = self.stream.snapshot() {
                if snapshot.render.is_some() {
                    self.retained = snapshot.render;
                }
            }
            self.retained
        }
        fn final_check(&mut self) -> Result<()> {
            self.snapshot(true).map(|_| ())
        }
        fn print_native(&mut self) {
            println!("ASIO final software-prepared progress/raw native diagnostics={:?}; prepared frames are not audible progress and raw native nanoseconds are not QPC", self.stream.snapshot());
        }
    }
    impl Drop for Stream {
        fn drop(&mut self) {
            let _ = self.stream.stop();
        }
    }
    pub(super) fn open(options: &Options, mixer: Mixer) -> Result<Box<dyn NativeOutput>> {
        let view = match options.asio_view.ok_or("ASIO view required")? {
            AsioView::Native => AsioRegistryView::Native,
            AsioView::Bits32 => AsioRegistryView::Bits32,
            AsioView::Bits64 => AsioRegistryView::Bits64,
        };
        let registrations = enumerate_asio_drivers(view, AsioEnumerationLimits::default())?;
        let mut matches = registrations
            .iter()
            .filter(|driver| driver.id.clsid == options.device);
        let registration = matches
            .next()
            .ok_or("selected ASIO CLSID was not registered in the explicit view")?;
        if matches.next().is_some() {
            return Err("ASIO CLSID has ambiguous registrations".into());
        }
        let window = Window::new()?;
        // SAFETY: explicitly selected installed driver is trusted; window is valid,
        // owned by this thread and retained until stream stop/Release/callback drain.
        let mut control = unsafe { AsioControl::open(registration, Some(window.hwnd as usize)) }?;
        let constraints = control.buffer_constraints()?;
        let request = options.buffer.map_or(
            AsioBufferRequest::DriverPreferred,
            AsioBufferRequest::Frames,
        );
        let resolved = constraints.resolve(request)?;
        if resolved as usize > AudioLimits::MAX_RENDER_FRAMES {
            return Err("ASIO driver buffer exceeds core render ceiling".into());
        }
        let channels = options
            .output_channels
            .clone()
            .ok_or("ASIO output channels required")?;
        println!("ASIO exact registration={registration:?}; requested buffer={request:?}; reported={constraints:?}; resolved frames={resolved}; reported rate={}; Mixer rate={}", control.sample_rate()?, options.format.sample_rate());
        for channel in &channels {
            println!(
                "ASIO selected native output={:?}",
                control.channel_info(*channel, false)?
            );
        }
        let host_clock = beatkernel_platform::windows::clock::QpcClock::new(
            beatkernel::time::ClockDomainId(1),
        )?;
        let stream = AsioStream::prepare_with_clock(control, mixer, channels, request, host_clock)?;
        Ok(Box::new(Stream {
            stream,
            window,
            retained: None,
        }))
    }
}
