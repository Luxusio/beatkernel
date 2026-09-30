use std::{
    error::Error,
    io::{self, BufRead},
};

use beatkernel::{
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceCapabilities, DeviceDescriptor,
        DeviceId, DeviceSelector, DeviceTransport, EventMeta, GameControlId, PhysicalControlId,
        PhysicalInputEvent, VirtualInputBackend,
    },
    interaction::{HoldEvaluator, InstantEvaluator, InteractionEvaluator},
    judge::{JudgeEngine, JudgeEvent, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeWindow, Rule},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};

const HOST_ORIGIN: i64 = 1_000_000_000;
const FINISH_SONG: i64 = 1_600_000_001;
const SOURCE: DeviceId = DeviceId(1);
const CLOCK: ClockDomainId = ClockDomainId(1);
const HELP: &str = "BeatKernel four-control judge example
Usage: cargo run -p beatkernel --example judge -- --help|--fixture|--stdin
Virtual input only; no native keyboard acquisition, audio or latency measurement.
Chart: lane 1 Instant at 500ms; lane 2 Hold 500..1500ms;
       lane 3 Instant at 1000ms; lane 4 Instant at 1500ms.
Grades: 1 within +/-20ms, 2 within +/-100ms (inclusive); offset 0.
Stdin: one 'host_ns lane down|up|repeat' per line; lane is 1..4.
Host origin 1000000000ns maps to song 0; timestamps must not decrease.
Example: 1500000000 1 down
Blank lines are ignored. EOF advances past the chart deadlines to report misses.
Malformed input exits nonzero with its line number.";

struct SameClock;
impl ClockMapper for SameClock {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}

fn judge() -> Result<JudgeEngine, Box<dyn Error>> {
    let mut chart = SourceChart::new(1000, Bpm::new(60, 1)?)?;
    for (id, start, end) in [
        (1, 500, None),
        (2, 500, Some(1500)),
        (3, 1000, None),
        (4, 1500, None),
    ] {
        chart.objects.push(SourceObject {
            id: ObjectId(id),
            start: Beat::new(start)?,
            end: end.map(Beat::new).transpose()?,
            interaction: InteractionId(id as u32),
            visual: VisualId(id as u32),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    let rules = (1..=4)
        .map(|lane| Rule {
            interaction: InteractionId(lane),
            control: GameControlId(lane),
            evaluator: if lane == 2 {
                Box::new(HoldEvaluator) as Box<dyn InteractionEvaluator>
            } else {
                Box::new(InstantEvaluator)
            },
        })
        .collect();
    let windows = [(1, 20_000_000), (2, 100_000_000)].map(|(grade, bound)| JudgeWindow {
        grade: JudgeGrade(grade),
        early: Duration::from_nanos(bound),
        late: Duration::from_nanos(bound),
    });
    Ok(JudgeEngine::new(
        chart.compile()?,
        rules,
        JudgeProfile::new(windows.to_vec(), Duration::ZERO)?,
    )?)
}

fn print_events(events: Vec<JudgeEvent>) {
    for event in events {
        let outcome = match event.outcome {
            JudgeOutcome::Hit { grade, delta } => {
                format!("hit grade={} delta_ns={}", grade.0, delta.as_nanos())
            }
            JudgeOutcome::Miss { reason } => format!("miss reason={reason:?}"),
        };
        let provenance = event.input.map_or_else(
            || "input=none".into(),
            |meta| {
                format!(
                    "source={} host_ns={} clock={} sequence={}",
                    meta.source.0,
                    meta.timestamp.as_nanos(),
                    meta.clock_domain.0,
                    meta.sequence
                )
            },
        );
        println!(
            "object={} stage={:?} song_ns={} {outcome} {provenance}",
            event.object.0,
            event.stage,
            event.at.as_nanos()
        );
    }
}

fn parse(line: &str) -> Result<(i64, u16, ButtonState), String> {
    let fields: Vec<_> = line.split_whitespace().collect();
    if fields.len() != 3 {
        return Err("expected host_ns lane down|up|repeat".into());
    }
    let host = fields[0]
        .parse::<i64>()
        .map_err(|_| "host_ns must be a signed 64-bit integer")?;
    let lane = fields[1].parse::<u16>().map_err(|_| "lane must be 1..4")?;
    if !(1..=4).contains(&lane) {
        return Err("lane must be 1..4".into());
    }
    let state = match fields[2] {
        "down" => ButtonState::Down,
        "up" => ButtonState::Up,
        "repeat" => ButtonState::Repeat,
        _ => return Err("state must be down, up or repeat".into()),
    };
    Ok((host, lane, state))
}

fn run(input: impl BufRead) -> Result<(), Box<dyn Error>> {
    let mut judge = judge()?;
    let transport = Transport::new(
        Timestamp::from_nanos(HOST_ORIGIN),
        Timestamp::ZERO,
        Rate::NORMAL,
    );
    let mut backend = VirtualInputBackend::new(CLOCK);
    backend.register_device(DeviceDescriptor {
        runtime_id: SOURCE,
        vendor_id: None,
        product_id: None,
        serial: None,
        name: Some("virtual four-control keyboard".into()),
        transport: DeviceTransport::Virtual,
        capabilities: DeviceCapabilities {
            button: true,
            ..Default::default()
        },
    })?;
    let bindings = BindingMap::from_bindings((1..=4).map(|lane| Binding {
        device: DeviceSelector::Exact(SOURCE),
        physical: PhysicalControlId::keyboard(lane + 3),
        game_control: GameControlId(u32::from(lane)),
    }))?;
    let mut last_host = HOST_ORIGIN;
    let mut last_song = 0;
    println!("Virtual four-control playback; host_origin_ns={HOST_ORIGIN}");
    for (index, line) in input.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let (host, lane, state) =
            parse(&line).map_err(|error| format!("line {}: {error}", index + 1))?;
        if host < last_host {
            return Err(format!(
                "line {}: host timestamp decreased or precedes origin {HOST_ORIGIN}",
                index + 1
            )
            .into());
        }
        let host_time = Timestamp::from_nanos(host);
        backend.push(
            PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(
                    SOURCE,
                    ClockPoint {
                        domain: CLOCK,
                        timestamp: host_time,
                    },
                    index as u64 + 1,
                ),
                control: PhysicalControlId::keyboard(lane + 3),
                state,
            }),
            &SameClock,
        )?;
        for canonical in backend.drain_events() {
            let song = transport.position_at(canonical.meta().timestamp)?;
            for bound in bindings.map(&canonical) {
                print_events(judge.push_input(&bound, song)?);
            }
            last_song = song.as_nanos();
        }
        last_host = host;
    }
    print_events(judge.advance_to(Timestamp::from_nanos(last_song.max(FINISH_SONG)))?);
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => {
            println!("{HELP}");
            Ok(())
        }
        [arg] if arg == "--help" => {
            println!("{HELP}");
            Ok(())
        }
        [arg] if arg == "--stdin" => run(io::stdin().lock()),
        [arg] if arg == "--fixture" => {
            println!("Synthetic fixture (no native I/O)");
            run(io::Cursor::new("1500000000 1 down\n1500000000 2 down\n1550000000 1 up\n2000000000 3 down\n2050000000 3 up\n2200000000 2 repeat\n2500000000 2 up\n2500000000 4 down\n"))
        }
        _ => Err("expected exactly one of --help, --fixture or --stdin".into()),
    }
}
