//! Bounded round-trip checks over the production wire codecs.
//!
//! Input and replay carry IEEE float bits, so their oracle compares canonical
//! encoded bytes rather than value equality (NaNs are not reflexive).

use beatkernel::input::codec::{decode_event, encode_event, CodecLimits};
use beatkernel::replay::codec::{decode_replay, encode_replay, ReplayCodecLimits};
use beatkernel_bms_runtime::multiplayer_room_wire::{decode_message, encode_message};

pub const INPUT_MAX_BYTES: usize = 65_536;
pub const INPUT_MAX_PAYLOAD: usize = 4_096;
pub const REPLAY_MAX_BYTES: usize = 65_536;
pub const REPLAY_MAX_RECORDS: usize = 64;
pub const REPLAY_MAX_HEADER: usize = 4_096;
// Includes the full production Join frame: 65_536 identity bytes, 64 players,
// and its header/length fields (65_808 bytes in total).
pub const ROOM_MAX_BYTES: usize = 131_072;

/// Campaign limits, independent of the production caller's choice of budgets.
pub fn input_limits() -> CodecLimits {
    CodecLimits::new(INPUT_MAX_BYTES, INPUT_MAX_PAYLOAD)
        .expect("fixed input campaign limits are valid")
}

/// File, record, header and nested physical-input campaign budgets.
pub fn replay_limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(
        REPLAY_MAX_BYTES,
        REPLAY_MAX_RECORDS,
        REPLAY_MAX_HEADER,
        input_limits(),
    )
    .expect("fixed replay campaign limits are valid")
}

/// Returns true only when the primary production input decoder accepts data.
/// Decoder errors and inputs outside campaign budgets are legitimate rejects.
pub fn check_input(data: &[u8]) -> bool {
    if data.len() > INPUT_MAX_BYTES {
        return false;
    }
    let limits = input_limits();
    let Ok(event) = decode_event(data, limits) else {
        return false;
    };
    let canonical = encode_event(&event, limits).expect("decoded input must encode");
    assert!(canonical.len() <= INPUT_MAX_BYTES);
    let decoded = decode_event(&canonical, limits).expect("canonical input must decode");
    let repeated = encode_event(&decoded, limits).expect("canonical input must re-encode");
    assert_eq!(canonical, repeated, "input canonical bytes must be stable");
    true
}

/// Returns true only when the primary production replay decoder accepts data.
/// Nested input values retain raw float bits through the canonical fixed point.
pub fn check_replay(data: &[u8]) -> bool {
    if data.len() > REPLAY_MAX_BYTES {
        return false;
    }
    let limits = replay_limits();
    let Ok(file) = decode_replay(data, limits) else {
        return false;
    };
    assert!(file.records.len() <= REPLAY_MAX_RECORDS);
    let canonical = encode_replay(&file, limits).expect("decoded replay must encode");
    assert!(canonical.len() <= REPLAY_MAX_BYTES);
    let decoded = decode_replay(&canonical, limits).expect("canonical replay must decode");
    let repeated = encode_replay(&decoded, limits).expect("canonical replay must re-encode");
    assert_eq!(canonical, repeated, "replay canonical bytes must be stable");
    true
}

/// Checks typed room equality and canonical bytes without granting authority.
pub fn check_room(data: &[u8]) -> bool {
    if data.len() > ROOM_MAX_BYTES {
        return false;
    }
    let Ok(message) = decode_message(data) else {
        return false;
    };
    let canonical = encode_message(&message).expect("decoded room message must encode");
    assert!(canonical.len() <= ROOM_MAX_BYTES);
    let decoded = decode_message(&canonical).expect("canonical room message must decode");
    assert_eq!(message, decoded, "room values must round-trip");
    let repeated = encode_message(&decoded).expect("canonical room message must re-encode");
    assert_eq!(canonical, repeated, "room canonical bytes must be stable");
    true
}
