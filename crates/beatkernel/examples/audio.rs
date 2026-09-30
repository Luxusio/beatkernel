use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits,
        PcmSample, SampleBank, SampleId, VoiceId,
    },
    time::{ClockDomainId, Timestamp},
};
use std::error::Error;

const HELP: &str = "BeatKernel deterministic offline audio fixture
Usage: cargo run -p beatkernel --example audio -- [--fixture|--help]
No arguments runs the fixture. Synthetic PCM only; no native device output.
The fixture checks literal mixing, scheduled Play/Stop offsets and partition invariance.";
const EXPECTED: [f32; 8] = [0.0, 0.0, 0.25, 0.25, 1.0, 0.5, 0.0, 0.0];

fn fixture_mixer() -> Result<Mixer, Box<dyn Error>> {
    let format = AudioFormat::new(1000, 1)?;
    let pcm_limits = PcmLimits::new(1024, 4096, 4)?;
    let mut bank = SampleBank::new(format, pcm_limits)?;
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, 0.75, 0.5], pcm_limits)?,
    )?;
    bank.insert(
        SampleId(2),
        PcmSample::new(format, vec![-0.25, 0.25, 0.5], pcm_limits)?,
    )?;
    let limits = AudioLimits::new(4, 2, 4, 16, 4)?;
    let (mut producer, consumer) = command_queue(4)?;
    for command in [
        AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::from_nanos(2_000_000),
            gain: 1.0,
        },
        AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(2),
            at: Timestamp::from_nanos(3_000_000),
            gain: 1.0,
        },
        AudioCommand::Stop {
            voice: VoiceId(1),
            at: Timestamp::from_nanos(5_000_000),
        },
    ] {
        producer
            .try_push(command)
            .map_err(|error| format!("fixture queue admission failed: {:?}", error.reason))?;
    }
    Ok(Mixer::new(
        MixerConfig::new(format, ClockDomainId(1), Timestamp::ZERO, limits),
        bank,
        consumer,
    )?)
}

fn run_fixture() -> Result<(), Box<dyn Error>> {
    let mut whole = fixture_mixer()?;
    let mut output = [0.0; 8];
    let report = whole.render(&mut output)?;
    let mut partitioned = fixture_mixer()?;
    let mut split = [0.0; 8];
    partitioned.render(&mut split[..2])?;
    // An empty render observes state without consuming more commands.
    partitioned.render(&mut [])?;
    partitioned.render(&mut split[2..5])?;
    partitioned.render(&mut split[5..])?;
    if output != EXPECTED
        || split != EXPECTED
        || report.counters.commands_applied != 3
        || report.counters.late_commands != 0
        || report.active_voices != 0
        || whole.frame_cursor() != partitioned.frame_cursor()
        || whole.counters() != partitioned.counters()
    {
        return Err("literal scheduled PCM fixture failed".into());
    }
    println!("synthetic offline audio fixture; no native playback evidence");
    println!("sample_rate=1000 channels=1 output={output:?}");
    println!(
        "frames={} commands_applied={} late_commands={} partition_invariance=PASS",
        report.frames, report.counters.commands_applied, report.counters.late_commands
    );
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => run_fixture(),
        [argument] if argument == "--fixture" => run_fixture(),
        [argument] if argument == "--help" => {
            println!("{HELP}");
            Ok(())
        }
        _ => Err("expected only --fixture or --help; run with --help".into()),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("audio: {error}");
        std::process::exit(1);
    }
}
