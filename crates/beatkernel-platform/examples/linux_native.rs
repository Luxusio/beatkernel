//! Explicit Linux native acquisition/output example; never opens a default device.
#[cfg(target_os = "linux")]
mod native {
    use beatkernel::{
        audio::{
            command_queue, AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits,
            PcmSample, SampleBank, SampleId, VoiceId,
        },
        input::DeviceId,
        time::{ClockDomainId, Timestamp},
    };
    use beatkernel_platform::{
        audio::{DeviceFormat, SampleEncoding},
        linux::{AlsaRequest, AlsaStream, EvdevDevice, EvdevItem, HidrawDevice},
    };
    use std::{
        error::Error,
        time::{Duration as StdDuration, Instant},
    };

    const HELP: &str = "BeatKernel explicit Linux native example\nUsage:\n  linux_native input /dev/input/eventN [seconds]\n  linux_native hidraw /dev/hidrawN [seconds]\n  linux_native audio ALSA_ENDPOINT RATE CHANNELS PERIOD_FRAMES BUFFER_FRAMES SECONDS\nNo arguments prints help. Input seconds defaults to 5 (max 60).\nAudio settings are exact: no size rounding, format float32 little endian.\nDevices must be explicitly selected and accessible. Native execution/latency is not verified by building this example.";

    pub fn run() -> Result<(), Box<dyn Error>> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.is_empty() || args == ["--help"] {
            println!("{HELP}");
            return Ok(());
        }
        match args[0].as_str() {
            "input" | "hidraw" if args.len() == 2 || args.len() == 3 => {
                let seconds = if args.len() == 3 {
                    args[2].parse::<u64>()?
                } else {
                    5
                };
                if !(1..=60).contains(&seconds) {
                    return Err("seconds must be 1..60".into());
                }
                let deadline = Instant::now() + StdDuration::from_secs(seconds);
                if args[0] == "input" {
                    let mut device = EvdevDevice::open(&args[1], DeviceId(1), ClockDomainId(1))?;
                    println!(
                        "device={:?} initial_state={:?}",
                        device.descriptor(),
                        device.query_state()?
                    );
                    while Instant::now() < deadline {
                        match device.read_next()? {
                            EvdevItem::WouldBlock => {
                                std::thread::sleep(StdDuration::from_millis(1))
                            }
                            EvdevItem::Resync(snapshot) => {
                                println!("loss barrier: host must reconcile keys/axes before gameplay resumes: {snapshot:?}");
                                // Inspector has no held gameplay state; printing the loss snapshot
                                // reconciles its observational state before acknowledgment.
                                device.acknowledge_resync();
                            }
                            EvdevItem::Ignored => {}
                            item => println!("{item:?}"),
                        }
                    }
                    println!("counters={:?}", device.counters());
                } else {
                    let mut device = HidrawDevice::open(&args[1], DeviceId(1), ClockDomainId(1))?;
                    println!(
                        "device={:?} descriptor={:02x?}",
                        device.descriptor(),
                        device.report_descriptor()
                    );
                    while Instant::now() < deadline {
                        match device.read_report()? {
                            Some(report) => println!("{report:?}"),
                            None => std::thread::sleep(StdDuration::from_millis(1)),
                        }
                    }
                    println!("counters={:?}", device.counters());
                }
            }
            "audio" if args.len() == 7 => {
                let rate = args[2].parse::<u32>()?;
                let channels = args[3].parse::<u16>()?;
                let period = args[4].parse::<u32>()?;
                let buffer = args[5].parse::<u32>()?;
                let seconds = args[6].parse::<u64>()?;
                if !(1..=60).contains(&seconds) {
                    return Err("seconds must be 1..60".into());
                }
                let format = AudioFormat::new(rate, channels)?;
                let limits = PcmLimits::new(4_000_000, 4_000_000, 1)?;
                let mut bank = SampleBank::new(format, limits)?;
                let mut tone = Vec::new();
                let frames = rate / 4;
                let len = (frames as usize)
                    .checked_mul(channels as usize)
                    .ok_or("tone size overflow")?;
                if len > 1_000_000 {
                    return Err("requested tone exceeds example asset budget".into());
                }
                tone.reserve_exact(len);
                for frame in 0..frames {
                    let value =
                        ((frame as f32) * std::f32::consts::TAU * 440.0 / rate as f32).sin() * 0.1;
                    for _ in 0..channels {
                        tone.push(value);
                    }
                }
                bank.insert(SampleId(1), PcmSample::new(format, tone, limits)?)?;
                let (mut producer, consumer) = command_queue(8)?;
                producer
                    .try_push(AudioCommand::Play {
                        voice: VoiceId(1),
                        sample: SampleId(1),
                        at: Timestamp::ZERO,
                        gain: 1.0,
                    })
                    .map_err(|error| format!("audio queue: {:?}", error.reason))?;
                let mixer = Mixer::new(
                    MixerConfig::new(
                        format,
                        ClockDomainId(2),
                        Timestamp::ZERO,
                        AudioLimits::new(8, 2, 8, 65_536, 8)?,
                    ),
                    bank,
                    consumer,
                )?;
                let request = AlsaRequest {
                    device: args[1].clone(),
                    format: DeviceFormat::new(rate, channels, SampleEncoding::Float32, None)?,
                    period_frames: period,
                    buffer_frames: buffer,
                    allow_size_rounding: false,
                    monotonic_domain: ClockDomainId(1),
                };
                let mut stream = AlsaStream::open(request, mixer)?;
                println!("applied={:?}", stream.configuration());
                let playback = (|| -> Result<(), Box<dyn Error>> {
                    stream.start()?;
                    std::thread::sleep(StdDuration::from_secs(seconds));
                    println!(
                        "native_timing_before_stop={:?}; estimated sound frames, physical latency unmeasured",
                        stream.timing_snapshot()
                    );
                    Ok(())
                })();
                let result = stream.stop();
                match stream.last_render_report() {
                    Some(report) => println!("last successful Mixer render report={report:?}; execution counters do not prove native write/presentation or physical sound"),
                    None => println!("last successful Mixer render report unavailable; no zero observation substituted"),
                }
                println!("observations={:?}", stream.snapshot());
                println!("native_timing_after_stop={:?}", stream.timing_snapshot());
                playback?;
                result?;
                drop(producer);
            }
            _ => return Err(format!("invalid arguments\n{HELP}").into()),
        }
        Ok(())
    }
}
#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::run()
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("linux_native requires Linux; no fallback device is opened");
    std::process::exit(1);
}
