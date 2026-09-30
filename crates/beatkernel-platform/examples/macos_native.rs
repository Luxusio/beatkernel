//! Finite owner-thread IOHID acquisition and explicit CoreAudio output setup.
#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use beatkernel::{
        audio::{AudioFormat, AudioLimits, Mixer, MixerConfig, SampleBank, command_queue},
        input::DeviceId,
        time::{ClockDomainId, Timestamp},
    };
    use beatkernel_platform::input::hid_report::NativeReportLayout;
    use beatkernel_platform::macos::{
        audio::{CoreAudioRequest, CoreAudioStream},
        clock::MachClock,
        input::HidInput,
    };
    use std::time::{Duration, Instant};
    let clock = MachClock::new(ClockDomainId(1), ClockDomainId(2))?;
    let devices = CoreAudioStream::devices()?;
    for device in &devices {
        println!(
            "CoreAudio device {} {:?}, rate {}, buffer {}, channels {}",
            device.id,
            device.name,
            device.nominal_rate,
            device.buffer_frames,
            device.output_channels
        );
    }
    let arguments: Vec<_> = std::env::args().collect();
    if arguments.len() != 5 && arguments.len() != 8 {
        println!(
            "usage: macos_native DEVICE_ID SAMPLE_RATE CHANNELS BUFFER_FRAMES [--raw separate|leading MAX_REPORT_BYTES]\nChoose an explicit enumerated audio device. Default HID scalar values; --raw explicitly selects timestamped native reports and declared ID layout."
        );
        return Ok(());
    }
    let raw_options = if arguments.len() == 8 {
        if arguments[5] != "--raw" {
            return Err("expected --raw".into());
        }
        let layout = match arguments[6].as_str() {
            "separate" => NativeReportLayout::SeparateId,
            "leading" => NativeReportLayout::LeadingId,
            _ => return Err("raw layout must be separate or leading".into()),
        };
        Some((layout, arguments[7].parse::<usize>()?))
    } else {
        None
    };
    let request = CoreAudioRequest {
        device: arguments[1].parse()?,
        format: AudioFormat::new(arguments[2].parse()?, arguments[3].parse()?)?,
        buffer_frames: arguments[4].parse()?,
    };
    let limits = AudioLimits::new(64, 16, 64, request.buffer_frames as usize, 64)?;
    let (_producer, consumer) = command_queue(limits.queue_capacity())?;
    // Output grid has an explicit separate identity. Pair presentation snapshots
    // with clock samples to establish the mapping before scheduling host input.
    let config = MixerConfig::new(request.format, ClockDomainId(3), Timestamp::ZERO, limits);
    let bank = SampleBank::new(
        request.format,
        beatkernel::audio::PcmLimits::new(4_194_304, 16_777_216, 16)?,
    );
    let mixer = Mixer::new(config, bank?, consumer)?;
    let mut audio = CoreAudioStream::open(request, clock, mixer)?;
    let mut input = if let Some((_, bytes)) = raw_options {
        HidInput::open_reports(clock, DeviceId(1), 1024, bytes)?
    } else {
        HidInput::open(clock, DeviceId(1), 1024)?
    };
    println!(
        "applied {:?}; HID devices {:?}",
        audio.configuration(),
        input.devices()
    );
    let outcome = (|| -> Result<(), Box<dyn std::error::Error>> {
        audio.start()?;
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            input.poll(Duration::from_millis(10))?;
            if let Some((layout, bytes)) = raw_options {
                while let Some(report) = input.pop_report() {
                    println!("native raw {report:?}");
                    println!(
                        "explicit-layout canonical {:?}",
                        report.to_raw_report(layout, bytes)
                    );
                    // A host may attach descriptors to the portable DeviceAdapterRegistry
                    // and route this canonical report through an explicitly registered
                    // vendor decoder. This example intentionally selects no vendor.
                }
            } else {
                while let Some(sample) = input.pop() {
                    println!("{sample:?}");
                }
            }
        }
        Ok(())
    })();
    println!(
        "presentation {:?}; input counters {:?}",
        audio.snapshot(),
        input.counters()
    );
    let stop = audio.stop();
    let close = input.close();
    println!(
        "stopped_render_cadence={:?}; actual pre-Mixer mach intervals, not native presentation or acoustic jitter",
        audio.render_cadence()
    );
    match audio.last_render_report() {
        Some(report) => println!(
            "last successful Mixer render report={report:?}; core execution distinct from native buffer delivery/presentation and physical sound"
        ),
        None => println!(
            "last successful Mixer render report unavailable; no zero observation substituted"
        ),
    }
    if let Err(error) = &stop {
        eprintln!("CoreAudio stop error: {error}");
    }
    if let Err(error) = &close {
        eprintln!("IOHID close error: {error}");
    }
    outcome?;
    stop?;
    close?;
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macos_native requires a macOS host and native IOHID/CoreAudio permissions");
}
