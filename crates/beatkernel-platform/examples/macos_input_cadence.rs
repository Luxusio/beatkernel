//! Selected IOHID scalar cadence with preserved native timestamp provenance.
#[cfg(target_os = "macos")]
mod native {
    use beatkernel::{
        input::{DeviceId, PhysicalControlId, PhysicalInputEvent},
        telemetry::{InputDeliveryTelemetry, IntervalJitter},
        time::{ClockDomainId, ClockPoint, Duration},
    };
    use beatkernel_platform::macos::{
        clock::MachClock,
        input::{HidInput, HidSample},
    };
    use std::{
        error::Error,
        time::{Duration as WallDuration, Instant},
    };

    const HOST: ClockDomainId = ClockDomainId(2);
    const CAPACITY: usize = 65_536;
    const HELP: &str = "Native macOS IOHID scalar cadence\nUsage:\n  macos_input_cadence --list\n  macos_input_cadence REGISTRY_ENTRY ELEMENT_COOKIE USAGE_PAGE USAGE INTEGER_VALUE NOMINAL_NS SECONDS\nUnsigned identity/period numbers are decimal; INTEGER_VALUE is signed decimal.\nNominal ns 1..i64::MAX, duration 1..60 seconds. Explicit device and scalar signal required.\nUse macos_native to inspect scalar cookies and values. Input Monitoring permission may be required.\nMeasures IOHID native mach timestamps and separate receipt age, not physical latency.\nNo generated input, inferred loss count, default device or per-event printing.";

    fn decimal(value: &str) -> Result<u64, Box<dyn Error>> {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("expected unsigned decimal integer".into());
        }
        Ok(value.parse()?)
    }
    pub fn run() -> Result<(), Box<dyn Error>> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args.is_empty() || args == ["--help"] {
            println!("{HELP}");
            return Ok(());
        }
        if args == ["--list"] {
            let clock = MachClock::new(ClockDomainId(1), HOST)?;
            let mut input = HidInput::open(clock, DeviceId(1), CAPACITY)?;
            let devices = input.devices();
            let close = input.close();
            for device in devices {
                println!("{device:?}");
            }
            close?;
            return Ok(());
        }
        if args.len() != 7 {
            return Err(HELP.into());
        }
        let registry = decimal(&args[0])?;
        let cookie = u32::try_from(decimal(&args[1])?)?;
        let usage_page = u16::try_from(decimal(&args[2])?)?;
        let usage = u16::try_from(decimal(&args[3])?)?;
        let integer_text = args[4].strip_prefix('-').unwrap_or(&args[4]);
        decimal(integer_text)?;
        let integer: i64 = args[4].parse()?;
        let nominal = i64::try_from(decimal(&args[5])?)?;
        let seconds = decimal(&args[6])?;
        if registry == 0
            || usage_page == 0
            || usage == 0
            || nominal <= 0
            || !(1..=60).contains(&seconds)
        {
            return Err(
                "nonzero registry/usage identities, positive nominal ns and seconds 1..60 required"
                    .into(),
            );
        }
        let nominal = Duration::from_nanos(nominal);
        let clock = MachClock::new(ClockDomainId(1), HOST)?;
        let mut jitter = IntervalJitter::new(CAPACITY, nominal, clock.sample()?.normalized)?;
        let mut age = InputDeliveryTelemetry::new(CAPACITY, HOST)?;
        let mut input = HidInput::open(clock, DeviceId(1), CAPACITY)?;
        let matches: Vec<_> = input
            .devices()
            .into_iter()
            .filter(|device| device.registry_entry == Some(registry))
            .collect();
        if matches.len() != 1 {
            input.close()?;
            return Err(
                "registry identity must select exactly one currently enumerated IOHID device"
                    .into(),
            );
        }
        let source = matches[0].descriptor.runtime_id;
        let control = PhysicalControlId::HidUsage { usage_page, usage };
        println!(
            "selected_device={:?} cookie={cookie} control={control:?} integer={integer} nominal_ns={} retention={CAPACITY}",
            matches[0],
            nominal.as_nanos()
        );
        println!(
            "event_clock=IOHID_mach_timestamp_normalized receipt_clock=shared_mach_origin; native accuracy is not established"
        );
        let initial_counters = input.counters();
        let deadline = Instant::now() + WallDuration::from_secs(seconds);
        let mut first: Option<HidSample> = None;
        let mut last: Option<HidSample> = None;
        let mut selected = 0u64;
        let mut other = 0u64;
        let mut timestamp_regressions = 0u64;
        let mut sequence_regressions = 0u64;
        let outcome = (|| -> Result<(), Box<dyn Error>> {
            while Instant::now() < deadline {
                input.poll(WallDuration::from_micros(100))?;
                let counters = input.counters();
                if counters.queue_full != initial_counters.queue_full {
                    return Err("IOHID queue loss: cadence segment terminated".into());
                }
                // Removal of any acquired device is a conservative discontinuity.
                // Reconnection can never silently replace the selected session ID.
                if counters.removed != initial_counters.removed {
                    return Err("IOHID device removal: cadence segment terminated".into());
                }
                while let Some(sample) = input.pop() {
                    if Instant::now() >= deadline {
                        break;
                    }
                    let sample_control = match &sample.event {
                        PhysicalInputEvent::Button(event) => event.control,
                        PhysicalInputEvent::Axis(event) => event.control,
                        _ => {
                            other = other.saturating_add(1);
                            continue;
                        }
                    };
                    let meta = *sample.event.meta();
                    if meta.source != source
                        || sample_control != control
                        || sample.element_cookie != cookie
                        || sample.integer_value != integer
                    {
                        other = other.saturating_add(1);
                        continue;
                    }
                    let receipt = clock.sample()?.normalized;
                    selected = selected.saturating_add(1);
                    let point = ClockPoint {
                        domain: meta.clock_domain,
                        timestamp: meta.timestamp,
                    };
                    if let Some(previous) = &last {
                        let previous = previous.event.meta();
                        if meta.sequence <= previous.sequence {
                            sequence_regressions = sequence_regressions.saturating_add(1);
                            return Err("selected source sequence did not increase".into());
                        }
                        if meta.timestamp < previous.timestamp {
                            timestamp_regressions = timestamp_regressions.saturating_add(1);
                            return Err("selected IOHID timestamp regressed".into());
                        }
                        age.observe(point, receipt)?;
                        jitter.observe(point)?;
                    } else {
                        age.observe(point, receipt)?;
                        jitter.reset(nominal, point)?;
                        first = Some(sample.clone());
                    }
                    last = Some(sample);
                }
            }
            Ok(())
        })();
        let counters = input.counters();
        let close = input.close();
        drop(input);
        println!(
            "native_counters={counters:?} selected={selected} nonselected={other} timestamp_regressions={timestamp_regressions} sequence_regressions={sequence_regressions}"
        );
        println!("first_actual_sample={first:?}\nlast_accepted_sample={last:?}");
        println!(
            "accepted_pairs={} retained_interval_summary={:?}",
            jitter.observed_pairs(),
            jitter.summary()?
        );
        println!(
            "accepted_delivery_ages={} retained_delivery_age_ns={:?}",
            age.observed_events(),
            age.summary()
        );
        println!("exact_missing_events=unknown physical_input_to_audio_latency=unavailable");
        if let Err(error) = &close {
            eprintln!("IOHID cleanup failed: {error}");
        }
        outcome?;
        close?;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::run()
}
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macos_input_cadence requires native macOS IOHID access");
}
