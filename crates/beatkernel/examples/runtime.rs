//! Deterministic canonical input -> binding -> judge -> actual PCM composition.
use beatkernel::{
    audio::{
        command_queue, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample,
        SampleBank, SampleId, VoiceId,
    },
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    interaction::InstantEvaluator,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow, Rule},
    runtime::{Runtime, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use std::error::Error;

struct FixtureClocks;
impl ClockMapper for FixtureClocks {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        match (from.domain.0, to.0) {
            (10, 20) => from
                .timestamp
                .checked_add(Duration::from_nanos(1_000_000_000)),
            (20, 30) => from
                .timestamp
                .checked_sub(Duration::from_nanos(1_000_000_000)),
            _ => None,
        }
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}

fn fixture() -> Result<(), Box<dyn Error>> {
    let mut source = SourceChart::new(1000, Bpm::new(60, 1)?)?;
    source.objects.push(SourceObject {
        id: ObjectId(1),
        start: Beat::new(2)?,
        end: None,
        interaction: InteractionId(1),
        visual: VisualId(1),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    let judge = JudgeEngine::new(
        source.compile()?,
        vec![Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: Box::new(InstantEvaluator),
        }],
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )?,
    )?;
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Exact(DeviceId(7)),
        physical: PhysicalControlId::keyboard(4),
        game_control: GameControlId(1),
    }])?;
    let (producer, consumer) = command_queue(4)?;
    let mut runtime = Runtime::new(
        ClockDomainId(20),
        ClockDomainId(30),
        Transport::new(
            Timestamp::from_nanos(1_000_000_000),
            Timestamp::ZERO,
            Rate::NORMAL,
        ),
        bindings,
        judge,
        producer,
        vec![SoundBinding {
            object: ObjectId(1),
            stage: JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(1),
            gain: 1.0,
        }],
        128,
    )?;
    let event = PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(
            DeviceId(7),
            ClockPoint {
                domain: ClockDomainId(10),
                timestamp: Timestamp::from_nanos(2_000_000),
            },
            1,
        ),
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    });
    let report = runtime.process_input(
        event,
        &FixtureClocks,
        ClockPoint {
            domain: ClockDomainId(20),
            timestamp: Timestamp::from_nanos(1_003_000_000),
        },
    )?;
    if report.judge_error.is_some()
        || !report.audio_failures.is_empty()
        || report.judge_events.len() != 1
    {
        return Err("composition did not publish its hit".into());
    }
    let format = AudioFormat::new(1000, 1)?;
    let limits = PcmLimits::new(1024, 4096, 4)?;
    let mut bank = SampleBank::new(format, limits)?;
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, 0.25], limits)?,
    )?;
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(30),
            Timestamp::ZERO,
            AudioLimits::new(4, 2, 4, 16, 4)?,
        ),
        bank,
        consumer,
    )?;
    let mut pcm = [0.0; 8];
    let rendered = mixer.render(&mut pcm)?;
    if pcm != [0.0, 0.0, 0.0, 0.25, 0.5, 0.25, 0.0, 0.0] {
        return Err("scheduled PCM differs from literal fixture".into());
    }
    println!("software composition fixture: PCM={pcm:?}");
    println!(
        "judge={:?} processing={:?} counters={:?} mixer={:?}",
        report.judge_events,
        runtime.telemetry().processing(),
        runtime.telemetry().counters(),
        rendered.counters
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => fixture(),
        [arg] if arg == "--fixture" => fixture(),
        [arg] if arg == "--help" => {
            println!("Usage: cargo run -p beatkernel --example runtime -- [--fixture|--help]\nRenders deterministic offline PCM through the integrated loop. Processing percentiles measure software only; native latency and hardware verification are deferred.");
            Ok(())
        }
        _ => Err("expected --fixture or --help".into()),
    }
}
