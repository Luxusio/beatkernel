use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits,
        PcmSample, SampleBank, SampleId, VoiceId,
    },
    time::{ClockDomainId, Duration, Timestamp},
};
use beatkernel_platform::audio::{
    encode_pcm, AudioStreamMode, BufferRequest, DeviceFormat, PeriodRequest, SampleEncoding,
    SharedPeriodPolicy,
};
use std::{collections::BTreeMap, error::Error};

const HELP: &str = "BeatKernel native Windows audio example
Usage: windows_audio --help|--fixture|--list|--probe|--play [options]
Help and synthetic --fixture work on every host. Native actions require Windows.
Probe/play require --device ID --mode shared|exclusive; no device/mode fallback.
  --shared-period engine|default  (shared only; default engine)
  --rate N --channels N --encoding f32|pcm16|pcm24|pcm32
  --valid-bits N --channel-mask none|N|0xHEX
  --buffer default|frames:N|ns:N --period default|frames:N|ns:N
  --allow-rounding              (Exact sizing otherwise)
  --wake event|timer --poll-ms N (timer requires shared and explicit positive N)
  --mmcss off|low|normal|high|critical (play only; default normal)
  --seconds N                  (play only; finite 0<N<=60, default 2)
No format flags selects queried mix format; explicit flags override that reported base.
Buffer/period default explicitly accepts native defaults. Exclusive numeric requests
must match; shared legacy requires period default. Independent legacy shared buffers
require explicitly selecting --shared-period default --wake timer --poll-ms N.
Probe buffer bounds describe event-driven queries; timer play queries its own bounds.
Native output requires submitted frames beyond prefill and device clock progression.
Counters labeled inferred are not hardware underrun or physical latency measurements.
ASIO is unavailable until its licensing/distribution path is resolved.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Help,
    Fixture,
    List,
    Probe,
    Play,
}

struct Args {
    action: Action,
    values: BTreeMap<String, String>,
    rounding: bool,
}

impl Args {
    fn parse() -> Result<Self, Box<dyn Error>> {
        let mut arguments = std::env::args().skip(1);
        let mut action = None;
        let mut values = BTreeMap::new();
        let mut rounding = false;
        while let Some(argument) = arguments.next() {
            let next_action = match argument.as_str() {
                "--help" => Some(Action::Help),
                "--fixture" => Some(Action::Fixture),
                "--list" => Some(Action::List),
                "--probe" => Some(Action::Probe),
                "--play" => Some(Action::Play),
                _ => None,
            };
            if let Some(next_action) = next_action {
                if action.replace(next_action).is_some() {
                    return Err("choose exactly one action; duplicate actions are invalid".into());
                }
                continue;
            }
            if argument == "--allow-rounding" {
                if rounding {
                    return Err("duplicate --allow-rounding".into());
                }
                rounding = true;
                continue;
            }
            let name = argument
                .strip_prefix("--")
                .ok_or_else(|| format!("unexpected positional argument {argument:?}"))?;
            if ![
                "device",
                "mode",
                "shared-period",
                "rate",
                "channels",
                "encoding",
                "valid-bits",
                "channel-mask",
                "buffer",
                "period",
                "wake",
                "poll-ms",
                "mmcss",
                "seconds",
            ]
            .contains(&name)
            {
                return Err(format!("unknown option {argument}; run --help").into());
            }
            if values.contains_key(name) {
                return Err(format!("duplicate {argument}").into());
            }
            let value = arguments
                .next()
                .filter(|value| !value.starts_with("--"))
                .ok_or_else(|| format!("missing value for {argument}"))?;
            values.insert(name.to_owned(), value);
        }
        let result = Self {
            action: action.unwrap_or(Action::Help),
            values,
            rounding,
        };
        result.validate()?;
        Ok(result)
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    fn validate(&self) -> Result<(), Box<dyn Error>> {
        if matches!(self.action, Action::Help | Action::Fixture | Action::List) {
            if !self.values.is_empty() || self.rounding {
                return Err("native options require --probe or --play".into());
            }
            return Ok(());
        }
        let device = self.get("device").ok_or("--device ID is required")?;
        if device.is_empty() || device.contains('\0') {
            return Err("--device must be a nonempty identity without NUL".into());
        }
        let mode = self.mode()?;
        if mode == AudioStreamMode::Exclusive && self.get("shared-period").is_some() {
            return Err("--shared-period applies only to --mode shared".into());
        }
        if let Some(rate) = self.get("rate") {
            positive_u32(rate, "--rate")?;
        }
        if let Some(channels) = self.get("channels") {
            if positive_u32(channels, "--channels")? > 32 {
                return Err("--channels must be 1..=32".into());
            }
        }
        if let Some(encoding) = self.get("encoding") {
            encoding_bits(encoding)?;
        }
        if let Some(bits) = self.get("valid-bits") {
            let bits = positive_u32(bits, "--valid-bits")?;
            if bits > 32 {
                return Err("--valid-bits must be 1..=32".into());
            }
            if let Some(encoding) = self.get("encoding") {
                let width = encoding_bits(encoding)?;
                if bits > u32::from(width) || (encoding == "f32" && bits != 32) {
                    return Err("--valid-bits is incompatible with --encoding".into());
                }
            }
        }
        if let Some(mask) = self.get("channel-mask") {
            let mask = channel_mask(mask)?;
            if let (Some(mask), Some(channels)) = (mask, self.get("channels")) {
                if mask != 0 && mask.count_ones() != positive_u32(channels, "--channels")? {
                    return Err("nonzero --channel-mask must assign one bit per channel".into());
                }
            }
        }
        if let (Some(rate), Some(channels), Some(encoding)) =
            (self.get("rate"), self.get("channels"), self.get("encoding"))
        {
            let encoding = if encoding == "f32" {
                SampleEncoding::Float32
            } else {
                let container_bits = encoding_bits(encoding)?;
                SampleEncoding::Pcm {
                    container_bits,
                    valid_bits: self
                        .get("valid-bits")
                        .map(|bits| positive_u32(bits, "--valid-bits"))
                        .transpose()?
                        .unwrap_or(u32::from(container_bits))
                        as u16,
                }
            };
            // Validate every already-known explicit field before host dispatch.
            // Omitted layout is unknown here; native selection still preserves
            // the queried mix mask unless the caller explicitly overrides it.
            DeviceFormat::new(
                positive_u32(rate, "--rate")?,
                positive_u32(channels, "--channels")? as u16,
                encoding,
                self.get("channel-mask")
                    .map(channel_mask)
                    .transpose()?
                    .flatten(),
            )?;
        }
        let buffer = self.buffer()?;
        let period = self.period()?;
        if mode == AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault)
            && period != PeriodRequest::DeviceDefault
        {
            return Err("legacy shared --shared-period default requires --period default".into());
        }
        if mode == AudioStreamMode::Exclusive {
            let mismatch = match (buffer, period) {
                (BufferRequest::Frames(buffer), PeriodRequest::Frames(period)) => buffer != period,
                (BufferRequest::Duration(buffer), PeriodRequest::Duration(period)) => {
                    buffer != period
                }
                (BufferRequest::Frames(buffer), PeriodRequest::Duration(period)) => self
                    .get("rate")
                    .map(|rate| {
                        positive_u32(rate, "--rate").map(|rate| {
                            i128::from(buffer) * 1_000_000_000
                                != i128::from(period.as_nanos()) * i128::from(rate)
                        })
                    })
                    .transpose()?
                    .unwrap_or(false),
                (BufferRequest::Duration(buffer), PeriodRequest::Frames(period)) => self
                    .get("rate")
                    .map(|rate| {
                        positive_u32(rate, "--rate").map(|rate| {
                            i128::from(period) * 1_000_000_000
                                != i128::from(buffer.as_nanos()) * i128::from(rate)
                        })
                    })
                    .transpose()?
                    .unwrap_or(false),
                _ => false,
            };
            if mismatch {
                return Err("exclusive numeric --buffer and --period must match".into());
            }
        }
        match self.get("wake").unwrap_or("event") {
            "event" if self.get("poll-ms").is_none() => {}
            "event" => return Err("--poll-ms requires --wake timer".into()),
            "timer" => {
                if mode == AudioStreamMode::Exclusive {
                    return Err("timer wake requires shared mode".into());
                }
                positive_u32(
                    self.get("poll-ms")
                        .ok_or("--wake timer requires --poll-ms N")?,
                    "--poll-ms",
                )?;
            }
            _ => return Err("--wake must be event or timer".into()),
        }
        if let Some(priority) = self.get("mmcss") {
            if !["off", "low", "normal", "high", "critical"].contains(&priority) {
                return Err("--mmcss must be off, low, normal, high or critical".into());
            }
        }
        self.seconds()?;
        if self.action == Action::Probe
            && (self.get("seconds").is_some() || self.get("mmcss").is_some())
        {
            return Err("--seconds and --mmcss apply only to --play".into());
        }
        Ok(())
    }

    fn mode(&self) -> Result<AudioStreamMode, Box<dyn Error>> {
        match self
            .get("mode")
            .ok_or("--mode shared|exclusive is required")?
        {
            "exclusive" => Ok(AudioStreamMode::Exclusive),
            "shared" => match self.get("shared-period").unwrap_or("engine") {
                "engine" => Ok(AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod)),
                "default" => Ok(AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault)),
                _ => Err("--shared-period must be engine or default".into()),
            },
            _ => Err("--mode must be shared or exclusive".into()),
        }
    }

    fn buffer(&self) -> Result<BufferRequest, Box<dyn Error>> {
        match size(self.get("buffer").unwrap_or("default"))? {
            Size::Default => Ok(BufferRequest::DeviceDefault),
            Size::Frames(frames) => Ok(BufferRequest::Frames(frames)),
            Size::Time(time) => Ok(BufferRequest::Duration(time)),
        }
    }

    fn period(&self) -> Result<PeriodRequest, Box<dyn Error>> {
        match size(self.get("period").unwrap_or("default"))? {
            Size::Default => Ok(PeriodRequest::DeviceDefault),
            Size::Frames(frames) => Ok(PeriodRequest::Frames(frames)),
            Size::Time(time) => Ok(PeriodRequest::Duration(time)),
        }
    }

    fn seconds(&self) -> Result<f64, Box<dyn Error>> {
        let seconds: f64 = self.get("seconds").unwrap_or("2").parse()?;
        if !seconds.is_finite() || seconds <= 0.0 || seconds > 60.0 {
            return Err("--seconds must be finite and greater than 0, at most 60".into());
        }
        if std::time::Duration::from_secs_f64(seconds).is_zero() {
            return Err("--seconds must represent at least one nanosecond".into());
        }
        Ok(seconds)
    }
}

enum Size {
    Default,
    Frames(u32),
    Time(Duration),
}

fn size(value: &str) -> Result<Size, Box<dyn Error>> {
    if value == "default" {
        return Ok(Size::Default);
    }
    if let Some(value) = value.strip_prefix("frames:") {
        return Ok(Size::Frames(positive_u32(value, "frame request")?));
    }
    if let Some(value) = value.strip_prefix("ns:") {
        let nanos: i64 = value.parse()?;
        if nanos > 0 {
            return Ok(Size::Time(Duration::from_nanos(nanos)));
        }
    }
    Err("size must be default, frames:positive_u32 or ns:positive_i64".into())
}

fn positive_u32(value: &str, option: &str) -> Result<u32, Box<dyn Error>> {
    let value: u32 = value
        .parse()
        .map_err(|_| format!("{option} requires a positive u32"))?;
    if value == 0 {
        return Err(format!("{option} must be positive").into());
    }
    Ok(value)
}

fn encoding_bits(value: &str) -> Result<u16, Box<dyn Error>> {
    match value {
        "f32" | "pcm32" => Ok(32),
        "pcm16" => Ok(16),
        "pcm24" => Ok(24),
        _ => Err("--encoding must be f32, pcm16, pcm24 or pcm32".into()),
    }
}

fn channel_mask(value: &str) -> Result<Option<u32>, Box<dyn Error>> {
    if value == "none" {
        return Ok(None);
    }
    let mask = if let Some(hex) = value.strip_prefix("0x") {
        u32::from_str_radix(hex, 16)?
    } else {
        value.parse()?
    };
    if mask & !0x0003_ffff != 0 {
        return Err("--channel-mask contains undefined speaker bits".into());
    }
    Ok(Some(mask))
}

fn fixture() -> Result<(), Box<dyn Error>> {
    let format = AudioFormat::new(1000, 1)?;
    let pcm_limits = PcmLimits::new(1024, 1024, 1)?;
    let mut bank = SampleBank::new(format, pcm_limits)?;
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, -0.25], pcm_limits)?,
    )?;
    let (mut producer, consumer) = command_queue(2)?;
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::from_nanos(1_000_000),
            gain: 1.0,
        })
        .map_err(|error| format!("fixture admission failed: {:?}", error.reason))?;
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(2, 1, 2, 16, 2)?,
        ),
        bank,
        consumer,
    )?;
    let mut pcm = [0.0; 4];
    let report = mixer.render(&mut pcm)?;
    let device_format = DeviceFormat::new(
        1000,
        1,
        SampleEncoding::Pcm {
            container_bits: 16,
            valid_bits: 16,
        },
        None,
    )?;
    let mut packed = [0u8; 8];
    encode_pcm(device_format, &pcm, &mut packed)?;
    if pcm != [0.0, 0.25, -0.25, 0.0] || packed != [0, 0, 0, 32, 0, 224, 0, 0] {
        return Err("portable literal PCM fixture failed".into());
    }
    println!("synthetic portable WASAPI preparation fixture; no native playback evidence");
    println!(
        "pcm={pcm:?} pcm16_le={packed:?} frames={} literal_check=PASS",
        report.frames
    );
    Ok(())
}

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use beatkernel_platform::{
        audio::{
            resolve_period, AudioBackendKind, AudioDeviceId, AudioOutputBackend, AudioOutputStream,
            AudioStreamRequest, AudioStreamSnapshot, AudioStreamStatus, FormatSupport,
            NegotiationPolicy,
        },
        windows::{
            audio::{WasapiBackend, WasapiOptions, WasapiPriority, WasapiWakePolicy},
            clock::QpcClock,
        },
    };
    use std::{
        thread,
        time::{Duration as StdDuration, Instant},
    };

    fn selected_format(args: &Args, mix: DeviceFormat) -> Result<DeviceFormat, Box<dyn Error>> {
        let rate = args
            .get("rate")
            .map(|value| positive_u32(value, "--rate"))
            .transpose()?
            .unwrap_or(mix.sample_rate());
        let channels = args
            .get("channels")
            .map(|value| positive_u32(value, "--channels"))
            .transpose()?
            .unwrap_or(u32::from(mix.channels())) as u16;
        let encoding = match args.get("encoding") {
            Some("f32") => SampleEncoding::Float32,
            Some(value) => {
                let container_bits = encoding_bits(value)?;
                let valid_bits = args
                    .get("valid-bits")
                    .map(|value| positive_u32(value, "--valid-bits"))
                    .transpose()?
                    .unwrap_or(u32::from(container_bits)) as u16;
                SampleEncoding::Pcm {
                    container_bits,
                    valid_bits,
                }
            }
            None => match (mix.encoding(), args.get("valid-bits")) {
                (SampleEncoding::Pcm { container_bits, .. }, Some(value)) => SampleEncoding::Pcm {
                    container_bits,
                    valid_bits: positive_u32(value, "--valid-bits")? as u16,
                },
                (SampleEncoding::Float32, Some(value))
                    if positive_u32(value, "--valid-bits")? != 32 =>
                {
                    return Err(
                        "queried float32 mix format requires 32 valid bits; select PCM explicitly"
                            .into(),
                    )
                }
                _ => mix.encoding(),
            },
        };
        let mask = args
            .get("channel-mask")
            .map(channel_mask)
            .transpose()?
            .unwrap_or(mix.channel_mask());
        Ok(DeviceFormat::new(rate, channels, encoding, mask)?)
    }

    fn options(args: &Args) -> Result<WasapiOptions, Box<dyn Error>> {
        let mmcss_priority = match args.get("mmcss").unwrap_or("normal") {
            "off" => None,
            "low" => Some(WasapiPriority::Low),
            "normal" => Some(WasapiPriority::Normal),
            "high" => Some(WasapiPriority::High),
            "critical" => Some(WasapiPriority::Critical),
            _ => unreachable!("portable validation checked MMCSS"),
        };
        let wake_policy = match args.get("wake").unwrap_or("event") {
            "event" => WasapiWakePolicy::EventDriven,
            _ => WasapiWakePolicy::Timer {
                poll_interval: Duration::from_nanos(
                    i64::from(positive_u32(
                        args.get("poll-ms").expect("validated timer interval"),
                        "--poll-ms",
                    )?) * 1_000_000,
                ),
            },
        };
        Ok(WasapiOptions {
            mmcss_priority,
            wake_policy,
        })
    }

    fn print_snapshot(label: &str, snapshot: AudioStreamSnapshot) {
        println!(
            "{label} status={:?} telemetry_available={}",
            snapshot.status, snapshot.telemetry_available
        );
        if snapshot.telemetry_available {
            println!("{label} submitted_frames={} buffer_fills={} padding_frames={} inferred_starvations={} inferred_deadline_misses={} native_failures={}",
                snapshot.counters.submitted_frames, snapshot.counters.buffer_fills, snapshot.counters.padding_frames,
                snapshot.counters.inferred_starvations, snapshot.counters.inferred_deadline_misses, snapshot.counters.native_failures);
            println!(
                "{label} clock={:?} render={:?}",
                snapshot.clock, snapshot.render
            );
        }
    }

    fn coherent(stream: &impl AudioOutputStream) -> Result<AudioStreamSnapshot, Box<dyn Error>> {
        for _ in 0..100 {
            let snapshot = stream.snapshot();
            if snapshot.telemetry_available {
                return Ok(snapshot);
            }
            thread::sleep(StdDuration::from_millis(1));
        }
        Err("coherent native telemetry unavailable after bounded retries".into())
    }

    fn play(args: &Args, request: AudioStreamRequest) -> Result<(), Box<dyn Error>> {
        let format = request.format().pcm();
        let frames = (format.sample_rate() / 10).max(1) as usize;
        let samples = frames
            .checked_mul(usize::from(format.channels()))
            .ok_or("tone sample extent overflow")?;
        let bytes = samples
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or("tone byte extent overflow")?;
        if bytes > 4 * 1024 * 1024 {
            return Err("requested format exceeds the example's 4 MiB tone preload limit".into());
        }
        let mut tone = Vec::new();
        tone.try_reserve_exact(samples)?;
        for frame in 0..frames {
            let sample = (std::f64::consts::TAU * 440.0 * frame as f64
                / f64::from(format.sample_rate()))
            .sin() as f32;
            for _ in 0..format.channels() {
                tone.push(sample);
            }
        }
        let pcm_limits = PcmLimits::new(4 * 1024 * 1024, 4 * 1024 * 1024, 1)?;
        let mut bank = SampleBank::new(format, pcm_limits)?;
        bank.insert(SampleId(1), PcmSample::new(format, tone, pcm_limits)?)?;
        let limits = AudioLimits::new(256, 8, 256, 1_048_576, 256)?;
        let config = MixerConfig::new(format, ClockDomainId(2), Timestamp::ZERO, limits);
        let (mut producer, consumer) = command_queue(256)?;
        let duration = StdDuration::from_secs_f64(args.seconds()?);
        let duration_ns = i64::try_from(duration.as_nanos())?;
        if duration_ns == 0 {
            return Err("--seconds must represent at least one nanosecond".into());
        }
        for (index, nanos) in (0..duration_ns).step_by(250_000_000).enumerate() {
            producer
                .try_push(AudioCommand::Play {
                    voice: VoiceId(index as u64),
                    sample: SampleId(1),
                    at: config
                        .origin()
                        .checked_add(Duration::from_nanos(nanos))
                        .ok_or("tone scheduling overflow")?,
                    gain: 0.1,
                })
                .map_err(|error| format!("tone admission failed: {:?}", error.reason))?;
        }
        let clock = QpcClock::new(ClockDomainId(1))?;
        println!("host_clock_origin={:?} output_origin_ns={} output_domain={} scheduled_tone_interval_ns=250000000 gain=0.1",
            clock.mapping(), config.origin().as_nanos(), config.domain().0);
        let mixer = Mixer::new(config, bank, consumer)?;
        let selected_options = options(args)?;
        println!("requested={request:?} worker_options={selected_options:?}");
        let mut stream = WasapiBackend.open(request, mixer, clock, selected_options)?;
        println!("applied={:?}", stream.configuration());
        let prefill = coherent(&stream)?;
        print_snapshot("prefill", prefill);
        let operation = (|| -> Result<(), Box<dyn Error>> {
            stream.start()?;
            let started = Instant::now();
            let mut submissions_progressed = false;
            let mut clock_progressed = false;
            while started.elapsed() < duration {
                let snapshot = stream.snapshot();
                match snapshot.status {
                    AudioStreamStatus::Running => {}
                    status => {
                        return Err(format!("native stream became terminal: {status:?}").into())
                    }
                }
                if snapshot.telemetry_available {
                    submissions_progressed |=
                        snapshot.counters.submitted_frames > prefill.counters.submitted_frames;
                    if let (Some(before), Some(now)) = (prefill.clock, snapshot.clock) {
                        clock_progressed |=
                            now.frequency == before.frequency && now.position > before.position;
                    }
                }
                thread::sleep(
                    StdDuration::from_millis(10).min(duration.saturating_sub(started.elapsed())),
                );
            }
            let final_running = coherent(&stream)?;
            print_snapshot("running", final_running);
            submissions_progressed |=
                final_running.counters.submitted_frames > prefill.counters.submitted_frames;
            if let (Some(before), Some(now)) = (prefill.clock, final_running.clock) {
                clock_progressed |=
                    now.frequency == before.frequency && now.position > before.position;
            }
            if !submissions_progressed || !clock_progressed {
                return Err(format!("native activity unproven: submissions_beyond_prefill={submissions_progressed} device_clock_progress={clock_progressed}").into());
            }
            Ok(())
        })();
        let stopped = stream.stop();
        drop(producer);
        let final_snapshot = stream.snapshot();
        print_snapshot("joined", final_snapshot);
        let cadence = stream.render_cadence();
        println!("joined Running render-entry QPC cadence={cadence:?}; Ready prefill excluded; not callback-arrival, native delivery or acoustic jitter");
        operation?;
        stopped?;
        cadence?;
        if final_snapshot.status != AudioStreamStatus::Stopped {
            return Err("worker did not report clean terminal stop".into());
        }
        println!("native_stream_activity=PASS; submitted PCM and clock progression verified; physical latency unmeasured");
        Ok(())
    }

    pub(super) fn run(args: &Args) -> Result<(), Box<dyn Error>> {
        let backend = WasapiBackend;
        if args.action == Action::List {
            let devices = backend.devices()?;
            println!("render_endpoints={} all_states=true", devices.len());
            for device in devices {
                println!("{device:?}");
            }
            return Ok(());
        }
        let device = AudioDeviceId(
            args.get("device")
                .expect("validated explicit identity")
                .to_owned(),
        );
        let mix = backend.mix_format(&device)?;
        let format = selected_format(args, mix)?;
        let overridden = ["rate", "channels", "encoding", "valid-bits", "channel-mask"]
            .iter()
            .any(|name| args.get(name).is_some());
        println!(
            "device={:?} queried_mix={mix:?} selected_format={format:?} selection_source={}",
            device,
            if overridden {
                "queried-mix-with-explicit-overrides"
            } else {
                "queried-mix"
            }
        );
        let mut request = AudioStreamRequest::new(
            device,
            AudioBackendKind::Wasapi,
            args.mode()?,
            format,
            args.buffer()?,
            args.period()?,
        )?;
        if args.rounding {
            request = request.with_negotiation(NegotiationPolicy::AllowSupportedRounding);
        }
        println!("requested={request:?}");
        let support = backend.supports_format(request.device(), request.mode(), format)?;
        println!("exact_format_probe={support:?}; closest_format_is_advisory=true");
        if let FormatSupport::Unsupported { closest } = support {
            return Err(format!("requested format is unsupported; advisory closest={closest:?}; choose a new explicit request").into());
        }
        let constraints = backend.period_constraints(request.device(), request.mode(), format)?;
        println!("constraints={constraints:?} buffer_bounds_query_wake=event");
        println!(
            "resolved_period={:?}",
            resolve_period(&request, constraints)?
        );
        if args.action == Action::Play {
            play(args, request)
        } else {
            Ok(())
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse()?;
    match args.action {
        Action::Help => {
            println!("{HELP}");
            Ok(())
        }
        Action::Fixture => fixture(),
        _ => {
            #[cfg(target_os = "windows")]
            {
                native::run(&args)
            }
            #[cfg(not(target_os = "windows"))]
            {
                Err("native --list/--probe/--play require Windows; --fixture is portable".into())
            }
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("windows_audio: {error}");
        std::process::exit(1);
    }
}
