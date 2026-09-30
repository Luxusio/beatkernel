//! Software-only Runtime -> logical projection -> external SVG composition.
//!
//! Run: `cargo run -p beatkernel --example runtime_visual -- [new-output.svg]`.
//! `--help` performs no fixture or file work. The default is runtime_visual.svg.
//! Twelve snapshots use actual canonical virtual inputs, JudgeEngine results,
//! Mixer reports and reusable RenderFrame storage. This is not native playback
//! or physical timing evidence. Existing output files are never overwritten.
use beatkernel::{
    audio::{
        command_queue, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample,
        SampleBank, SampleId, VoiceId,
    },
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        BackendId, Binding, BindingMap, ButtonEvent, ButtonState, DeviceCapabilities,
        DeviceDescriptor, DeviceId, DeviceSelector, DeviceTransport, EventMeta, GameControlId,
        PhysicalControlId, PhysicalInputEvent, PointerEvent, PointerMode, Position2,
        VirtualInputBackend,
    },
    interaction::{HoldEvaluator, InstantEvaluator, TrackingEvaluator, TrackingInput},
    judge::{
        JudgeEngine, JudgeEvent, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeStage, JudgeWindow,
        Rule,
    },
    runtime::{Runtime, RuntimeReport, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
    visual::{Projection, RenderFrame, RenderObjectState, VisualBinding, VisualProjector},
};
use std::{error::Error, fmt::Write as _, fs::OpenOptions, io::Write as _, path::PathBuf};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const HOST: ClockDomainId = ClockDomainId(1);
const OUTPUT: ClockDomainId = ClockDomainId(2);
const STEP_NS: i64 = 100_000_000;
const MAX_SVG_BYTES: usize = 256 * 1024;
struct SameDomainOnly;
impl ClockMapper for SameDomainOnly {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn point(domain: ClockDomainId, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain,
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn pointer_control() -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(1),
        code: 1,
    }
}
fn report_events(report: RuntimeReport, accumulated: &mut Vec<JudgeEvent>) -> Result<Timestamp> {
    accumulated.extend(report.judge_events);
    if let Some(error) = report.judge_error {
        return Err(error.into());
    }
    if !report.audio_failures.is_empty() {
        return Err(format!(
            "actual runtime audio admission failures: {:?}",
            report.audio_failures
        )
        .into());
    }
    Ok(report.song_time)
}
fn panel(
    svg: &mut String,
    step: usize,
    frame: &RenderFrame,
    projector: &VisualProjector,
) -> Result<()> {
    let x = (step % 4) * 300;
    let y = (step / 4) * 270;
    writeln!(svg,"<g transform=\"translate({x},{y})\"><rect x=\"4\" y=\"4\" width=\"292\" height=\"262\" fill=\"#172331\"/><text x=\"12\" y=\"23\" fill=\"white\">song={}ms; results={}</text><path d=\"M14 175H144\" stroke=\"white\"/>",frame.song_time.as_nanos()/1_000_000,frame.transient_events.len())?;
    for object in &frame.objects {
        match object {
            RenderObjectState::Lane {
                lane,
                distance,
                tail_distance,
                ..
            } => {
                let x = 24.0 + f64::from(*lane) * 30.0;
                let y = (175.0 - distance * 100.0).clamp(35.0, 205.0);
                if let Some(tail) = tail_distance {
                    let tail_y = (175.0 - tail * 100.0).clamp(35.0, 205.0);
                    writeln!(
                        svg,
                        "<path d=\"M{x} {y}V{tail_y}\" stroke=\"#59bdff\" stroke-width=\"7\"/>"
                    )?;
                }
                writeln!(
                    svg,
                    "<rect x=\"{}\" y=\"{}\" width=\"20\" height=\"5\" fill=\"#e3c34b\"/>",
                    x - 10.0,
                    y - 2.5
                )?;
            }
            RenderObjectState::Path {
                visual,
                head,
                progress,
                visible_start,
                visible_end,
                ..
            } => {
                if let Some(Projection::Path { points }) = projector.projection(*visual) {
                    svg.push_str("<polyline fill=\"none\" stroke=\"#70889e\" points=\"");
                    for position in points {
                        write!(
                            svg,
                            "{},{} ",
                            165.0 + position[0] * 110.0,
                            175.0 - position[1] * 110.0
                        )?;
                    }
                    svg.push_str("\"/>");
                }
                writeln!(svg,"<circle cx=\"{}\" cy=\"{}\" r=\"6\" fill=\"#7ee5b7\"/><text x=\"150\" y=\"200\" fill=\"white\" font-size=\"11\">path {:.2} visible {:.2}..{:.2}</text>",165.0+head[0]*110.0,175.0-head[1]*110.0,progress,visible_start,visible_end)?;
            }
            _ => return Err("fixture emitted an unexpected projection".into()),
        }
    }
    for (row, event) in frame.transient_events.iter().enumerate() {
        let label = match event.outcome {
            JudgeOutcome::Hit { .. } => "hit",
            JudgeOutcome::Miss { .. } => "miss",
        };
        writeln!(svg,"<text x=\"12\" y=\"{}\" fill=\"#e4edf5\" font-size=\"11\">object {} {:?}: {label}</text>",222+row*13,event.object.0,event.stage)?;
    }
    svg.push_str("</g>\n");
    if svg.len() > MAX_SVG_BYTES {
        return Err("SVG fixture exceeded bounded output extent".into());
    }
    Ok(())
}
fn render(path: PathBuf) -> Result<()> {
    let mut source = SourceChart::new(1000, Bpm::new(60, 1)?)?;
    for (id, start, end) in [
        (1, 200, None),
        (2, 400, None),
        (3, 300, Some(700)),
        (4, 600, None),
        (5, 200, Some(800)),
    ] {
        source.objects.push(SourceObject {
            id: ObjectId(id),
            start: Beat::new(start)?,
            end: end.map(Beat::new).transpose()?,
            interaction: InteractionId(id as u32),
            visual: VisualId(id as u32),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    let compiled = source.compile()?;
    let rules = vec![
        Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: Box::new(InstantEvaluator),
        },
        Rule {
            interaction: InteractionId(2),
            control: GameControlId(2),
            evaluator: Box::new(InstantEvaluator),
        },
        Rule {
            interaction: InteractionId(3),
            control: GameControlId(3),
            evaluator: Box::new(HoldEvaluator),
        },
        Rule {
            interaction: InteractionId(4),
            control: GameControlId(4),
            evaluator: Box::new(InstantEvaluator),
        },
        Rule {
            interaction: InteractionId(5),
            control: GameControlId(5),
            evaluator: Box::new(TrackingEvaluator {
                input: TrackingInput::Pointer,
                points: vec![[0.0, 0.0, 0.0], [0.5, 1.0, 0.0], [1.0, 0.0, 0.0]],
                tolerance: 0.02,
                max_gap: Duration::from_nanos(150_000_000),
            }),
        },
    ];
    let judge = JudgeEngine::new(
        compiled.clone(),
        rules,
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::from_nanos(10_000_000),
                late: Duration::from_nanos(10_000_000),
            }],
            Duration::ZERO,
        )?,
    )?;
    let mut geometry = (1..=4)
        .map(|id| VisualBinding {
            id: VisualId(id),
            projection: Projection::Lane {
                lane: id - 1,
                unit: Duration::from_nanos(500_000_000),
            },
        })
        .collect::<Vec<_>>();
    geometry.push(VisualBinding {
        id: VisualId(5),
        projection: Projection::Path {
            points: vec![[0.0, 0.0], [0.5, 1.0], [1.0, 0.0]],
        },
    });
    let projector = VisualProjector::new(compiled, geometry)?;
    let mut bindings = (1..=4)
        .map(|id| Binding {
            device: DeviceSelector::Exact(DeviceId(7)),
            physical: PhysicalControlId::keyboard(id as u16 + 3),
            game_control: GameControlId(id),
        })
        .collect::<Vec<_>>();
    bindings.push(Binding {
        device: DeviceSelector::Exact(DeviceId(7)),
        physical: pointer_control(),
        game_control: GameControlId(5),
    });
    let format = AudioFormat::new(1000, 1)?;
    let pcm_limits = PcmLimits::new(1024, 4096, 1)?;
    let mut bank = SampleBank::new(format, pcm_limits)?;
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 20], pcm_limits)?,
    )?;
    let (producer, consumer) = command_queue(16)?;
    let sounds = vec![
        (1, JudgeStage::Instant),
        (2, JudgeStage::Instant),
        (3, JudgeStage::HoldHead),
        (3, JudgeStage::HoldTail),
        (5, JudgeStage::Custom(0)),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (object, stage))| SoundBinding {
        object: ObjectId(object),
        stage,
        sample: SampleId(1),
        voice: VoiceId(index as u64 + 1),
        gain: 1.0,
    })
    .collect();
    let mut runtime = Runtime::new(
        HOST,
        OUTPUT,
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        BindingMap::from_bindings(bindings)?,
        judge,
        producer,
        sounds,
        128,
    )?;
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            OUTPUT,
            Timestamp::ZERO,
            AudioLimits::new(16, 8, 16, 100, 16)?,
        ),
        bank,
        consumer,
    )?;
    let mut input = VirtualInputBackend::new(HOST);
    input.register_device(DeviceDescriptor {
        runtime_id: DeviceId(7),
        vendor_id: None,
        product_id: None,
        serial: None,
        name: Some("software fixture keyboard/pointer".into()),
        transport: DeviceTransport::Virtual,
        capabilities: DeviceCapabilities {
            button: true,
            pointer: true,
            ..Default::default()
        },
    })?;
    let mut frame = RenderFrame::default();
    let mut events = Vec::with_capacity(8);
    let mut pcm = [0.0; 100];
    let mut sequence = 0u64;
    let mut svg=String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1200\" height=\"850\" viewBox=\"0 0 1200 850\"><rect width=\"1200\" height=\"850\" fill=\"#0e1722\"/><text x=\"12\" y=\"22\" fill=\"white\">Software fixture: actual Runtime reports; no native playback or physical timing claim</text><g transform=\"translate(0,30)\" font-family=\"monospace\" font-size=\"13\">\n");
    let mut final_render = None;
    for step in 0..12 {
        let now = step as i64 * STEP_NS;
        events.clear();
        let button = match step {
            2 => Some((4, ButtonState::Down)),
            3 => Some((6, ButtonState::Down)),
            4 => Some((5, ButtonState::Down)),
            7 => Some((6, ButtonState::Up)),
            _ => None,
        };
        if let Some((usage, state)) = button {
            sequence += 1;
            input.push(
                PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(7), point(HOST, now), sequence),
                    control: PhysicalControlId::keyboard(usage),
                    state,
                }),
                &SameDomainOnly,
            )?;
        }
        if (2..=8).contains(&step) {
            let progress = (step - 2) as f32 / 6.0;
            let y = if progress <= 0.5 {
                progress * 2.0
            } else {
                (1.0 - progress) * 2.0
            };
            sequence += 1;
            input.push(
                PhysicalInputEvent::Pointer(PointerEvent {
                    meta: EventMeta::new(DeviceId(7), point(HOST, now), sequence),
                    control: pointer_control(),
                    position: Position2 { x: progress, y },
                    mode: PointerMode::Absolute,
                }),
                &SameDomainOnly,
            )?;
        }
        while let Some(event) = input.pop() {
            let report = runtime.process_input(event, &SameDomainOnly, point(OUTPUT, now))?;
            let _actual_song = report_events(report, &mut events)?;
        }
        let song_time = report_events(
            runtime.advance_to(point(HOST, now), &SameDomainOnly, point(OUTPUT, now))?,
            &mut events,
        )?;
        // All projection time and effects come from accepted Runtime reports.
        projector.project(
            song_time,
            song_time
                .checked_sub(Duration::from_nanos(300_000_000))
                .ok_or("visible window overflow")?,
            song_time
                .checked_add(Duration::from_nanos(500_000_000))
                .ok_or("visible window overflow")?,
            &events,
            &mut frame,
        )?;
        panel(&mut svg, step, &frame, &projector)?;
        final_render = Some(mixer.render(&mut pcm)?);
    }
    svg.push_str("</g></svg>\n");
    if svg.len() > MAX_SVG_BYTES {
        return Err("SVG fixture output exceeds bound".into());
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    output.write_all(svg.as_bytes())?;
    output.flush()?;
    println!("software contact sheet={} snapshots=12 bytes={}; actual final Mixer report={final_render:?}; runtime counters={:?}",path.display(),svg.len(),runtime.telemetry().counters());
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [arg] if arg == "--help" => {
            println!("cargo run -p beatkernel --example runtime_visual -- [NEW_OUTPUT.svg]\nDefault runtime_visual.svg; create_new never overwrites. Twelve actual software-runtime snapshots; no native or physical claim.");
            Ok(())
        }
        [] => render(PathBuf::from("runtime_visual.svg")),
        [path] if !path.is_empty() && !path.starts_with('-') => render(PathBuf::from(path)),
        _ => Err("expected optional new SVG path or --help".into()),
    }
}
