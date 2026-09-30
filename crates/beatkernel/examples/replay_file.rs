//! Bounded actual replay-file load/save; reconstruction remains application-owned.
use beatkernel::{input::CodecLimits, replay::codec::*};
use std::{
    fs::File,
    io::{Read, Write},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "--help" {
        println!("replay_file INPUT.bkr NEW_OUTPUT.bkr\nBounded decode/re-encode preserving runtime/header/input/calibration data. No native playback or judge reconstruction.");
        return Ok(());
    }
    if args.len() != 2 {
        return Err("usage: replay_file INPUT.bkr NEW_OUTPUT.bkr".into());
    }
    let limits = ReplayCodecLimits::new(
        16 * 1024 * 1024,
        100_000,
        1024 * 1024,
        CodecLimits::new(1024 * 1024, 512 * 1024)?,
    )?;
    let mut bytes = Vec::new();
    File::open(&args[0])?
        .take(limits.max_file_bytes() as u64 + 1)
        .read_to_end(&mut bytes)?;
    let replay = decode_replay(&bytes, limits)?;
    let encoded = encode_replay(&replay, limits)?;
    let mut output = File::create_new(&args[1])?;
    output.write_all(&encoded)?;
    output.flush()?;
    println!("saved {} ordered operations, runtime {:?}, {} bytes; identities require application validation", replay.records.len(), replay.runtime_version, encoded.len());
    Ok(())
}
