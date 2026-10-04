//! Bounded whole-cohort progress payloads, independent of transport ownership.
//! These self-reported prefixes do not establish a start or final acknowledgement.

use crate::local_players::{PlayerId, MAX_LOCAL_PLAYERS};
use crate::multiplayer_protocol::{validate_progress, MultiplayerError, Progress};

const HEADER_BYTES: usize = 12;
const MEMBER_BYTES: usize = 44;

/// One member's actual committed frontier and cumulative counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemberProgress {
    pub player: PlayerId,
    pub progress: Progress,
}

/// An ordered cohort prefix with an explicit caller-owned sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupPrefix {
    pub sequence: u64,
    pub final_prefix: bool,
    pub members: Vec<MemberProgress>,
}

fn validate_player_ids(
    players: impl ExactSizeIterator<Item = PlayerId> + Clone,
) -> Result<(), MultiplayerError> {
    if players.len() == 0 || players.len() > MAX_LOCAL_PLAYERS {
        return Err(MultiplayerError::Protocol(
            "group progress requires 1..64 members".into(),
        ));
    }
    for (index, player) in players.clone().enumerate() {
        if player.0 == 0
            || players
                .clone()
                .take(index)
                .any(|previous| previous == player)
        {
            return Err(MultiplayerError::Protocol(
                "group progress requires positive unique player identities".into(),
            ));
        }
    }
    Ok(())
}

/// Validate a setup roster independently of any reported score payload.
pub fn validate_roster(players: &[PlayerId]) -> Result<(), MultiplayerError> {
    validate_player_ids(players.iter().copied())
}

fn validate_snapshot(members: &[MemberProgress]) -> Result<(), MultiplayerError> {
    validate_player_ids(members.iter().map(|member| member.player))?;
    for member in members {
        validate_progress(None, member.progress)?;
    }
    Ok(())
}

/// Validate both supplied snapshots before admitting any member's next prefix.
/// A previous snapshot fixes the exact roster order as well as scalar chronology.
pub fn validate_members(
    previous: Option<&[MemberProgress]>,
    members: &[MemberProgress],
) -> Result<(), MultiplayerError> {
    if let Some(previous) = previous {
        validate_snapshot(previous)?;
    }
    validate_snapshot(members)?;
    if let Some(previous) = previous {
        if previous.len() != members.len()
            || previous
                .iter()
                .zip(members)
                .any(|(before, after)| before.player != after.player)
        {
            return Err(MultiplayerError::Protocol(
                "group progress changed the ordered roster".into(),
            ));
        }
        for (before, after) in previous.iter().zip(members) {
            validate_progress(Some(before.progress), after.progress)?;
        }
    }
    Ok(())
}

/// Encode schema 1: final flag, member count, sequence, then 44-byte member rows.
/// The maximum payload is 2828 bytes; this does not add a transport frame.
pub fn encode_prefix(
    sequence: u64,
    final_prefix: bool,
    members: &[MemberProgress],
) -> Result<Vec<u8>, MultiplayerError> {
    validate_members(None, members)?;
    let mut payload = Vec::new();
    payload
        .try_reserve_exact(HEADER_BYTES + members.len() * MEMBER_BYTES)
        .map_err(|_| MultiplayerError::Protocol("group prefix allocation failed".into()))?;
    payload.push(1);
    payload.push(u8::from(final_prefix));
    payload.extend_from_slice(&(members.len() as u16).to_le_bytes());
    payload.extend_from_slice(&sequence.to_le_bytes());
    for member in members {
        payload.extend_from_slice(&member.player.0.to_le_bytes());
        payload.extend_from_slice(&member.progress.song_ns.to_le_bytes());
        for value in [
            member.progress.hits,
            member.progress.misses,
            member.progress.combo,
            member.progress.max_combo,
        ] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
    }
    Ok(payload)
}

fn read_word(bytes: &[u8]) -> Result<u64, MultiplayerError> {
    let bytes = bytes
        .try_into()
        .map_err(|_| MultiplayerError::Protocol("invalid group word extent".into()))?;
    Ok(u64::from_le_bytes(bytes))
}

/// Decode only the caller's expected sequence and a complete valid cohort.
/// The previous prefix is borrowed and never mutated on a rejected payload.
pub fn decode_prefix(
    payload: &[u8],
    expected_sequence: u64,
    previous: Option<&[MemberProgress]>,
) -> Result<GroupPrefix, MultiplayerError> {
    if payload.len() < HEADER_BYTES
        || payload.len() > HEADER_BYTES + MAX_LOCAL_PLAYERS * MEMBER_BYTES
    {
        return Err(MultiplayerError::Protocol(
            "invalid group prefix size".into(),
        ));
    }
    if payload[0] != 1 || payload[1] > 1 {
        return Err(MultiplayerError::Protocol(
            "invalid group prefix schema or final flag".into(),
        ));
    }
    let count = usize::from(u16::from_le_bytes([payload[2], payload[3]]));
    if count == 0
        || count > MAX_LOCAL_PLAYERS
        || payload.len() != HEADER_BYTES + count * MEMBER_BYTES
    {
        return Err(MultiplayerError::Protocol(
            "invalid group prefix member extent".into(),
        ));
    }
    let sequence = read_word(&payload[4..HEADER_BYTES])?;
    if sequence != expected_sequence {
        return Err(MultiplayerError::Protocol("invalid group sequence".into()));
    }
    let mut members = Vec::new();
    members
        .try_reserve_exact(count)
        .map_err(|_| MultiplayerError::Protocol("group member allocation failed".into()))?;
    for row in payload[HEADER_BYTES..].chunks_exact(MEMBER_BYTES) {
        members.push(MemberProgress {
            player: PlayerId(u32::from_le_bytes([row[0], row[1], row[2], row[3]])),
            progress: Progress {
                song_ns: read_word(&row[4..12])? as i64,
                hits: read_word(&row[12..20])?,
                misses: read_word(&row[20..28])?,
                combo: read_word(&row[28..36])?,
                max_combo: read_word(&row[36..44])?,
            },
        });
    }
    validate_members(previous, &members)?;
    Ok(GroupPrefix {
        sequence,
        final_prefix: payload[1] == 1,
        members,
    })
}

/// Eleven exact u32 words per member: player, then low/high song and counters.
/// Signed song time retains its two's-complement bits without floating conversion.
pub fn encode_words(members: &[MemberProgress]) -> Result<Vec<u32>, MultiplayerError> {
    validate_members(None, members)?;
    let mut words = Vec::new();
    words
        .try_reserve_exact(members.len() * 11)
        .map_err(|_| MultiplayerError::Protocol("group word allocation failed".into()))?;
    for member in members {
        words.push(member.player.0);
        for value in [
            member.progress.song_ns as u64,
            member.progress.hits,
            member.progress.misses,
            member.progress.combo,
            member.progress.max_combo,
        ] {
            words.extend_from_slice(&[value as u32, (value >> 32) as u32]);
        }
    }
    Ok(words)
}

/// Decode exact browser rows before admitting any group progress to a session.
pub fn decode_words(words: &[u32]) -> Result<Vec<MemberProgress>, MultiplayerError> {
    if words.is_empty() || words.len() > MAX_LOCAL_PLAYERS * 11 || words.len() % 11 != 0 {
        return Err(MultiplayerError::Protocol(
            "invalid group word extent".into(),
        ));
    }
    let mut members = Vec::new();
    members
        .try_reserve_exact(words.len() / 11)
        .map_err(|_| MultiplayerError::Protocol("group word decoding allocation failed".into()))?;
    for row in words.chunks_exact(11) {
        let value = |index| u64::from(row[index]) | (u64::from(row[index + 1]) << 32);
        members.push(MemberProgress {
            player: PlayerId(row[0]),
            progress: Progress {
                song_ns: value(1) as i64,
                hits: value(3),
                misses: value(5),
                combo: value(7),
                max_combo: value(9),
            },
        });
    }
    validate_members(None, &members)?;
    Ok(members)
}
