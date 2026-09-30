//! Finite logical replay demonstrating a real active hold checkpoint and seek.
use beatkernel::{chart::*, input::*, interaction::HoldEvaluator, judge::*, replay::*, time::*};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "--help" {
        println!("replay [--save NEW_FILE.bkr]\nFinite hold replay/checkpoint demo; optional save writes the complete ordered log, not the current inspection cursor.");
        return Ok(());
    }
    if !args.is_empty() && (args.len() != 2 || args[0] != "--save") {
        return Err("usage: replay [--save NEW_FILE.bkr]".into());
    }
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1)?)?;
    source.objects.push(SourceObject {
        id: ObjectId(1),
        start: Beat::new(100)?,
        end: Some(Beat::new(200)?),
        interaction: InteractionId(1),
        visual: VisualId(0),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::from_nanos(10),
            late: Duration::from_nanos(10),
        }],
        Duration::ZERO,
    )?;
    let engine = JudgeEngine::new(
        compile(&source)?,
        vec![Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: Box::new(HoldEvaluator),
        }],
        profile,
    )?;
    let header = ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: b"example-hold/v1".to_vec(),
        rules_identity: b"builtin-hold/v1".to_vec(),
        options: vec![],
        seed: 0,
        normalized_clock: ClockDomainId(1),
    };
    let mut replay = ReplaySession::new(header, engine)?;
    let input = GameInputEvent {
        game_control: GameControlId(1),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(
                DeviceId(1),
                ClockPoint {
                    domain: ClockDomainId(1),
                    timestamp: Timestamp::from_nanos(100),
                },
                1,
            ),
            control: PhysicalControlId::keyboard(4),
            state: ButtonState::Down,
        }),
    };
    replay.push_input(input, Timestamp::from_nanos(100))?;
    replay.checkpoint()?;
    replay.advance_to(Timestamp::from_nanos(300))?;
    replay.seek(Timestamp::from_nanos(150))?;
    println!(
        "at 150ns: {:?}, hash {:016x}",
        replay.engine().state(ObjectId(1)),
        replay.stable_hash()?
    );
    replay.seek(Timestamp::from_nanos(250))?;
    println!(
        "at 250ns: {:?}, results {:?}",
        replay.engine().state(ObjectId(1)),
        replay.results()
    );
    if let Some(path) = args.get(1) {
        use beatkernel::replay::codec::{encode_replay, ReplayCodecLimits, ReplayFile};
        use std::io::Write;
        let limits =
            ReplayCodecLimits::new(1024 * 1024, 1024, 64 * 1024, CodecLimits::new(4096, 1024)?)?;
        let file = ReplayFile::new(replay.header().clone(), replay.records().to_vec());
        let bytes = encode_replay(&file, limits)?;
        let mut output = std::fs::File::create_new(path)?;
        output.write_all(&bytes)?;
        output.flush()?;
        println!("saved {} admitted operations to {path}", file.records.len());
    }
    Ok(())
}
