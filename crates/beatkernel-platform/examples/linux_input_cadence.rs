//! Bounded selected-signal native cadence; execution is explicitly caller-owned.
#[cfg(target_os = "linux")]
mod native {
    use beatkernel::{
        input::{ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent},
        telemetry::{InputDeliveryTelemetry, IntervalJitter},
        time::{ClockDomainId, ClockPoint, Duration},
    };
    use beatkernel_platform::linux::{EvdevDevice, EvdevItem, HidrawDevice, MonotonicClock};
    use std::{
        error::Error,
        time::{Duration as WallDuration, Instant},
    };

    const DOMAIN: ClockDomainId = ClockDomainId(1);
    const CAPACITY: usize = 65_536;
    const HELP: &str = "Native Linux input cadence\nUsage:\n  linux_input_cadence evdev PATH HID_KEYBOARD_USAGE down|up|repeat NOMINAL_NS SECONDS\n  linux_input_cadence hidraw PATH none|REPORT_ID NOMINAL_NS SECONDS\nDecimal numbers only; nominal ns 1..i64::MAX, seconds 1..60.\nOne explicit device and signal; no generated input or default device.\nRetains at most 65536 pairs/ages. No per-record printing.\nEvdev timestamps are kernel CLOCK_MONOTONIC; hidraw timestamps are userspace receipt.\nLoss barriers and timestamp regression terminate the segment. Missing-event counts and physical latency are unknown.";

    enum Source {
        Evdev(EvdevDevice, PhysicalControlId, ButtonState),
        Hidraw(HidrawDevice, Option<u8>),
    }
    enum Read {
        Selected(EventMeta),
        Other,
        Idle,
        Loss,
    }
    impl Source {
        fn read(&mut self) -> Result<Read, Box<dyn Error>> {
            Ok(match self {
                Self::Evdev(device, control, state) => match device.read_next()? {
                    EvdevItem::Event(PhysicalInputEvent::Button(event))
                        if event.control == *control && event.state == *state =>
                    {
                        Read::Selected(event.meta)
                    }
                    EvdevItem::WouldBlock => Read::Idle,
                    EvdevItem::Dropped | EvdevItem::Resync(_) => Read::Loss,
                    _ => Read::Other,
                },
                Self::Hidraw(device, id) => match device.read_report()? {
                    Some(report) if report.report_id == *id => Read::Selected(report.meta),
                    Some(_) => Read::Other,
                    None => Read::Idle,
                },
            })
        }
        fn describe(&self) {
            match self {
                Self::Evdev(device, control, state) => println!(
                    "backend=evdev device={:?} control={control:?} state={state:?} event_clock=kernel_CLOCK_MONOTONIC",
                    device.descriptor()
                ),
                Self::Hidraw(device, id) => println!(
                    "backend=hidraw device={:?} report_id={id:?} event_clock=userspace_CLOCK_MONOTONIC_receipt",
                    device.descriptor()
                ),
            }
        }
        fn counters(&self) -> beatkernel_platform::linux::LinuxInputCounters {
            match self {
                Self::Evdev(device, ..) => device.counters(),
                Self::Hidraw(device, ..) => device.counters(),
            }
        }
    }
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
        let offset = match args.first().map(String::as_str) {
            Some("evdev") if args.len() == 6 => 4,
            Some("hidraw") if args.len() == 5 => 3,
            _ => return Err(HELP.into()),
        };
        let nominal = i64::try_from(decimal(&args[offset])?)?;
        let seconds = decimal(&args[offset + 1])?;
        if nominal <= 0 || !(1..=60).contains(&seconds) {
            return Err("nominal ns must be positive; seconds must be 1..60".into());
        }
        let nominal = Duration::from_nanos(nominal);
        let clock = MonotonicClock::new(DOMAIN);
        // Reserve both rings before native acquisition. The baseline is reset
        // to the first actual selected event, never the setup clock reading.
        let mut jitter = IntervalJitter::new(CAPACITY, nominal, clock.now()?)?;
        let mut age = InputDeliveryTelemetry::new(CAPACITY, DOMAIN)?;
        let mut source = if offset == 4 {
            let usage = u16::try_from(decimal(&args[2])?)?;
            if usage == 0 {
                return Err("HID keyboard usage must be nonzero".into());
            }
            let state = match args[3].as_str() {
                "down" => ButtonState::Down,
                "up" => ButtonState::Up,
                "repeat" => ButtonState::Repeat,
                _ => return Err("button state must be down, up or repeat".into()),
            };
            Source::Evdev(
                EvdevDevice::open(&args[1], DeviceId(1), DOMAIN)?,
                PhysicalControlId::keyboard(usage),
                state,
            )
        } else {
            let id = if args[2] == "none" {
                None
            } else {
                let id = u8::try_from(decimal(&args[2])?)?;
                if id == 0 {
                    return Err(
                        "numbered report ID must be nonzero; use none for unnumbered".into(),
                    );
                }
                Some(id)
            };
            Source::Hidraw(HidrawDevice::open(&args[1], DeviceId(1), DOMAIN)?, id)
        };
        source.describe();
        println!(
            "nominal_ns={} retention={CAPACITY}; interval deviation is not inferred loss or physical jitter",
            nominal.as_nanos()
        );
        let deadline = Instant::now() + WallDuration::from_secs(seconds);
        let mut first = None;
        let mut last = None;
        let mut selected = 0u64;
        let mut other = 0u64;
        let mut timestamp_regressions = 0u64;
        let mut sequence_regressions = 0u64;
        let mut loss_barriers = 0u64;
        let outcome = (|| -> Result<(), Box<dyn Error>> {
            while Instant::now() < deadline {
                match source.read()? {
                    Read::Idle => std::thread::sleep(WallDuration::from_micros(100)),
                    Read::Other => other = other.saturating_add(1),
                    Read::Loss => {
                        loss_barriers = loss_barriers.saturating_add(1);
                        return Err("evdev loss barrier: measurement segment terminated".into());
                    }
                    Read::Selected(meta) => {
                        let receipt = clock.now()?;
                        selected = selected.saturating_add(1);
                        let point = ClockPoint {
                            domain: meta.clock_domain,
                            timestamp: meta.timestamp,
                        };
                        if let Some(previous) = &last {
                            let previous: &EventMeta = previous;
                            if meta.sequence <= previous.sequence {
                                sequence_regressions = sequence_regressions.saturating_add(1);
                                return Err("selected source sequence did not increase".into());
                            }
                            if point.timestamp < previous.timestamp {
                                timestamp_regressions = timestamp_regressions.saturating_add(1);
                                return Err("selected event timestamp regressed".into());
                            }
                            jitter.observe(point)?;
                        } else {
                            jitter.reset(nominal, point)?;
                            first = Some(meta);
                        }
                        age.observe(point, receipt)?;
                        last = Some(meta);
                    }
                }
            }
            Ok(())
        })();
        // Close native acquisition before summary sorting/printing, including
        // partial failures. Preserve counters before dropping the handle.
        let counters = source.counters();
        drop(source);
        println!("native_counters={counters:?}");
        println!(
            "selected={selected} nonselected={other} loss_barriers={loss_barriers} timestamp_regressions={timestamp_regressions} sequence_regressions={sequence_regressions}"
        );
        println!("first_actual_metadata={first:?}\nlast_accepted_metadata={last:?}");
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
        outcome
    }
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("linux_input_cadence requires Linux evdev/hidraw access");
}
