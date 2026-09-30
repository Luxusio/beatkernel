//! Reads one actual bounded UTF-8 BMS file and compiles its gameplay/BGM timeline.
use beatkernel_bms::{parse, ParseOptions};
use std::{fs::File, io::Read};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: load_bms PATH.bms (UTF-8 text; assets remain unopened)")?;
    let options = ParseOptions::default();
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(options.max_bytes as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > options.max_bytes {
        return Err("BMS input byte limit exceeded".into());
    }
    let text = std::str::from_utf8(&bytes)?;
    let parsed = parse(text, options)?;
    let compiled = parsed.compile()?;
    println!(
        "title {:?}; resolution {}; objects {}; BGM {}; samples {}",
        parsed.metadata.get("TITLE"),
        parsed.source.ticks_per_beat,
        compiled.chart.objects().len(),
        compiled.bgm.len(),
        parsed.samples.len()
    );
    for object in compiled.chart.objects().iter().take(16) {
        println!(
            "object {} at {}ns end {:?} interaction {} audio {:?}",
            object.id.0,
            object.time.start.as_nanos(),
            object.time.end.map(|time| time.as_nanos()),
            object.interaction.0,
            object.audio
        );
    }
    for sound in compiled.bgm.iter().take(16) {
        println!(
            "BGM sample {} at {}ns (ordinal {})",
            sound.sample.0,
            sound.at.as_nanos(),
            sound.ordinal
        );
    }
    for warning in parsed.warnings {
        eprintln!("line {}: {}", warning.line, warning.message);
    }
    Ok(())
}
