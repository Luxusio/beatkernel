//! Bounded historical records, distinct from live completion evidence.
use crate::{
    gauge::{
        GaugeProfile, GaugeSnapshot, GaugeFailure, GradeDelta, MAX_GAUGE_UNITS, MAX_GAUGE_GRADES,
    },
    local_players::PlayerId,
    play_result::{CompletedPlayResult, PlayResultScope, PlayResultOutcome},
};
use beatkernel::{
    input::CodecLimits,
    judge::JudgeGrade,
    replay::{
        ReplayHeader,
        codec::{ReplayFile, ReplayCodecLimits, encode_replay, decode_replay},
    },
    time::Timestamp,
};

pub const VERSION: u32 = 1;
pub const MAX_PLAYERS: usize = 64;
/// Maximum complete embedded replay envelope (not just variable fields).
pub const MAX_HEADER_BYTES: usize = 65_536;
pub const MAX_ARCHIVE_BYTES: usize = 5 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"BKRESULT";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchivedResult {
    pub scope: PlayResultScope,
    pub outcome: PlayResultOutcome,
    pub gauge: GaugeSnapshot,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub player: PlayerId,
    pub header: ReplayHeader,
    pub profile: GaugeProfile,
    pub result: ArchivedResult,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultArchive {
    entries: Vec<ArchiveEntry>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArchiveError {
    Invalid(&'static str),
    UnsupportedVersion(u32),
    Truncated,
    TrailingBytes,
    TooLarge,
    AllocationFailed,
    Replay(beatkernel::replay::codec::ReplayCodecError),
}
impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "result archive: {self:?}")
    }
}
impl std::error::Error for ArchiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Replay(e) => Some(e),
            _ => None,
        }
    }
}
impl From<beatkernel::replay::codec::ReplayCodecError> for ArchiveError {
    fn from(e: beatkernel::replay::codec::ReplayCodecError) -> Self {
        Self::Replay(e)
    }
}
fn reserve<T>(v: &mut Vec<T>, count: usize) -> Result<(), ArchiveError> {
    v.try_reserve_exact(count)
        .map_err(|_| ArchiveError::AllocationFailed)
}
fn copy_bytes(bytes: &[u8]) -> Result<Vec<u8>, ArchiveError> {
    let mut v = Vec::new();
    reserve(&mut v, bytes.len())?;
    v.extend_from_slice(bytes);
    Ok(v)
}
fn copy_header(header: &ReplayHeader) -> Result<ReplayHeader, ArchiveError> {
    let size = header
        .chart_identity
        .len()
        .checked_add(header.rules_identity.len())
        .and_then(|size| size.checked_add(header.options.len()))
        .ok_or(ArchiveError::TooLarge)?;
    if size > MAX_HEADER_BYTES {
        return Err(ArchiveError::TooLarge);
    }
    Ok(ReplayHeader {
        version: header.version,
        chart_identity: copy_bytes(&header.chart_identity)?,
        rules_identity: copy_bytes(&header.rules_identity)?,
        options: copy_bytes(&header.options)?,
        seed: header.seed,
        normalized_clock: header.normalized_clock,
    })
}
fn copy_profile(profile: &GaugeProfile) -> Result<GaugeProfile, ArchiveError> {
    let mut grades = Vec::new();
    reserve(&mut grades, profile.grades().len())?;
    grades.extend_from_slice(profile.grades());
    GaugeProfile::new(
        profile.initial_units(),
        profile.clear_units(),
        profile.default_hit_delta(),
        profile.miss_delta(),
        profile.fail_on_empty(),
        grades,
    )
    .map_err(|_| ArchiveError::Invalid("gauge profile"))
}
fn replay_limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(
        MAX_HEADER_BYTES,
        1,
        MAX_HEADER_BYTES,
        CodecLimits::new(6, 0).expect("fixed input bounds"),
    )
    .expect("fixed replay bounds")
}
fn header_bytes(header: &ReplayHeader) -> Result<Vec<u8>, ArchiveError> {
    let mut runtime_version = String::new();
    runtime_version
        .try_reserve_exact(env!("CARGO_PKG_VERSION").len())
        .map_err(|_| ArchiveError::AllocationFailed)?;
    runtime_version.push_str(env!("CARGO_PKG_VERSION"));
    Ok(encode_replay(
        &ReplayFile {
            runtime_version,
            calibration_metadata: None,
            header: copy_header(header)?,
            records: Vec::new(),
        },
        replay_limits(),
    )?)
}
impl ResultArchive {
    /// Associates a whole original roster with actual immutable completion evidence.
    pub fn from_completed(
        rows: &[(PlayerId, CompletedPlayResult)],
        identities: &[(PlayerId, ReplayHeader, GaugeProfile)],
    ) -> Result<Self, ArchiveError> {
        if rows.is_empty() || rows.len() > MAX_PLAYERS || identities.len() != rows.len() {
            return Err(ArchiveError::Invalid("roster size"));
        }
        for (i, (id, _, _)) in identities.iter().enumerate() {
            if id.0 == 0 || identities[..i].iter().any(|(prior, _, _)| prior == id) {
                return Err(ArchiveError::Invalid("identity roster"));
            }
        }
        let mut entries = Vec::new();
        reserve(&mut entries, rows.len())?;
        for (player, result) in rows {
            let (_, header, profile) = identities
                .iter()
                .find(|(id, _, _)| id == player)
                .ok_or(ArchiveError::Invalid("missing identity"))?;
            entries.push(ArchiveEntry {
                player: *player,
                header: copy_header(header)?,
                profile: copy_profile(profile)?,
                result: ArchivedResult {
                    scope: result.scope(),
                    outcome: result.outcome(),
                    gauge: result.gauge(),
                },
            });
        }
        let archive = Self { entries };
        validate(&archive)?;
        Ok(archive)
    }
    /// Project one original historical member only after validating the whole archive.
    pub fn for_player(&self, player: PlayerId) -> Result<Self, ArchiveError> {
        validate(self)?;
        if player.0 == 0 {
            return Err(ArchiveError::Invalid("player identity"));
        }
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.player == player)
            .ok_or(ArchiveError::Invalid("unknown player identity"))?;
        let mut entries = Vec::new();
        reserve(&mut entries, 1)?;
        entries.push(ArchiveEntry {
            player: entry.player,
            header: copy_header(&entry.header)?,
            profile: copy_profile(&entry.profile)?,
            result: entry.result,
        });
        Ok(Self { entries })
    }
    pub fn entries(&self) -> &[ArchiveEntry] {
        &self.entries
    }
}
fn validate(archive: &ResultArchive) -> Result<(), ArchiveError> {
    let entries = &archive.entries;
    if entries.is_empty() || entries.len() > MAX_PLAYERS {
        return Err(ArchiveError::Invalid("roster size"));
    }
    for (index, entry) in entries.iter().enumerate() {
        if entry.player.0 == 0 || entries[..index].iter().any(|e| e.player == entry.player) {
            return Err(ArchiveError::Invalid("player identity"));
        }
        if entry.result.scope != entries[0].result.scope {
            return Err(ArchiveError::Invalid("common extent"));
        }
        let (start, end) = match entry.result.scope {
            PlayResultScope::FullSong => (Timestamp::ZERO, None),
            PlayResultScope::PracticeSection { start, end } => {
                if start < Timestamp::ZERO
                    || end.is_some_and(|end| end <= start)
                    || (start == Timestamp::ZERO && end.is_none())
                {
                    return Err(ArchiveError::Invalid("practice extent"));
                }
                (start, end)
            }
        };
        let setup = crate::replay_playback::decode_section_setup(&entry.header.options)
            .map_err(|_| ArchiveError::Invalid("replay setup"))?;
        if setup.start != start || setup.end != end {
            return Err(ArchiveError::Invalid("replay extent"));
        }
        // Canonical replay validation checks supported logical identities and bounds.
        header_bytes(&entry.header)?;
        let gauge = entry.result.gauge;
        if gauge.level_units > MAX_GAUGE_UNITS {
            return Err(ArchiveError::Invalid("gauge level"));
        }
        match gauge.failure {
            Some(GaugeFailure::InstantDeath) if gauge.level_units != 0 => {
                return Err(ArchiveError::Invalid("instant death level"));
            }
            Some(GaugeFailure::Depleted)
                if gauge.level_units != 0 || !entry.profile.fail_on_empty() =>
            {
                return Err(ArchiveError::Invalid("depletion policy"));
            }
            None if gauge.level_units == 0 && entry.profile.fail_on_empty() => {
                return Err(ArchiveError::Invalid("missing depletion"));
            }
            _ => {}
        }
        let expected = match gauge.failure {
            Some(failure) => PlayResultOutcome::Failed(failure),
            None if gauge.level_units >= entry.profile.clear_units() => PlayResultOutcome::Cleared,
            None => PlayResultOutcome::BelowClearThreshold,
        };
        if entry.result.outcome != expected {
            return Err(ArchiveError::Invalid("gauge outcome"));
        }
    }
    Ok(())
}
struct Writer(Vec<u8>);
impl Writer {
    fn put(&mut self, bytes: &[u8]) -> Result<(), ArchiveError> {
        if self
            .0
            .len()
            .checked_add(bytes.len())
            .is_none_or(|len| len > MAX_ARCHIVE_BYTES)
        {
            return Err(ArchiveError::TooLarge);
        }
        self.0
            .try_reserve(bytes.len())
            .map_err(|_| ArchiveError::AllocationFailed)?;
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}
pub fn encode_archive(archive: &ResultArchive) -> Result<Vec<u8>, ArchiveError> {
    validate(archive)?;
    let mut w = Writer(Vec::new());
    w.put(MAGIC)?;
    w.put(&VERSION.to_le_bytes())?;
    w.put(&(archive.entries.len() as u32).to_le_bytes())?;
    for e in &archive.entries {
        let header = header_bytes(&e.header)?;
        w.put(&e.player.0.to_le_bytes())?;
        w.put(&(header.len() as u32).to_le_bytes())?;
        w.put(&header)?;
        let p = &e.profile;
        w.put(&p.initial_units().to_le_bytes())?;
        w.put(&p.clear_units().to_le_bytes())?;
        w.put(&p.default_hit_delta().to_le_bytes())?;
        w.put(&p.miss_delta().to_le_bytes())?;
        w.put(&[u8::from(p.fail_on_empty())])?;
        w.put(&(p.grades().len() as u32).to_le_bytes())?;
        for grade in p.grades() {
            w.put(&grade.grade.0.to_le_bytes())?;
            w.put(&grade.delta.to_le_bytes())?;
        }
        match e.result.scope {
            PlayResultScope::FullSong => w.put(&[0])?,
            PlayResultScope::PracticeSection { start, end } => {
                w.put(&[1])?;
                w.put(&start.as_nanos().to_le_bytes())?;
                w.put(&[u8::from(end.is_some())])?;
                if let Some(end) = end {
                    w.put(&end.as_nanos().to_le_bytes())?;
                }
            }
        }
        w.put(&e.result.gauge.level_units.to_le_bytes())?;
        w.put(&[match e.result.gauge.failure {
            None => 0,
            Some(GaugeFailure::InstantDeath) => 1,
            Some(GaugeFailure::Depleted) => 2,
        }])?;
        w.put(&[match e.result.outcome {
            PlayResultOutcome::Cleared => 0,
            PlayResultOutcome::BelowClearThreshold => 1,
            PlayResultOutcome::Failed(GaugeFailure::InstantDeath) => 2,
            PlayResultOutcome::Failed(GaugeFailure::Depleted) => 3,
        }])?;
    }
    Ok(w.0)
}
struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], ArchiveError> {
        let end = self
            .cursor
            .checked_add(count)
            .ok_or(ArchiveError::TooLarge)?;
        let bytes = self
            .bytes
            .get(self.cursor..end)
            .ok_or(ArchiveError::Truncated)?;
        self.cursor = end;
        Ok(bytes)
    }
    fn byte(&mut self) -> Result<u8, ArchiveError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, ArchiveError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| ArchiveError::Truncated)?,
        ))
    }
    fn u64(&mut self) -> Result<u64, ArchiveError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| ArchiveError::Truncated)?,
        ))
    }
    fn i64(&mut self) -> Result<i64, ArchiveError> {
        Ok(i64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| ArchiveError::Truncated)?,
        ))
    }
}
pub fn decode_archive(bytes: &[u8]) -> Result<ResultArchive, ArchiveError> {
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(ArchiveError::TooLarge);
    }
    let mut r = Reader { bytes, cursor: 0 };
    if r.take(8)? != MAGIC {
        return Err(ArchiveError::Invalid("magic"));
    }
    let version = r.u32()?;
    if version != VERSION {
        return Err(ArchiveError::UnsupportedVersion(version));
    }
    let count = r.u32()? as usize;
    if count == 0 || count > MAX_PLAYERS {
        return Err(ArchiveError::Invalid("roster size"));
    }
    let mut entries = Vec::new();
    reserve(&mut entries, count)?;
    for _ in 0..count {
        let player = PlayerId(r.u32()?);
        let length = r.u32()? as usize;
        if length > MAX_HEADER_BYTES {
            return Err(ArchiveError::TooLarge);
        }
        let encoded = r.take(length)?;
        let file = decode_replay(encoded, replay_limits())?;
        if !file.records.is_empty() || file.calibration_metadata.is_some() {
            return Err(ArchiveError::Invalid("header-only replay"));
        }
        // Envelope producer version is not a gameplay identity; preserve only ReplayHeader.
        let initial = r.u64()?;
        let clear = r.u64()?;
        let hit = r.i64()?;
        let miss = r.i64()?;
        let fail = match r.byte()? {
            0 => false,
            1 => true,
            _ => return Err(ArchiveError::Invalid("failure flag")),
        };
        let grades_count = r.u32()? as usize;
        if grades_count > MAX_GAUGE_GRADES {
            return Err(ArchiveError::Invalid("grade count"));
        }
        let mut grades = Vec::new();
        reserve(&mut grades, grades_count)?;
        for _ in 0..grades_count {
            grades.push(GradeDelta {
                grade: JudgeGrade(r.u32()?),
                delta: r.i64()?,
            });
        }
        if grades.windows(2).any(|pair| pair[0].grade >= pair[1].grade) {
            return Err(ArchiveError::Invalid("grade order"));
        }
        let profile = GaugeProfile::new(initial, clear, hit, miss, fail, grades)
            .map_err(|_| ArchiveError::Invalid("gauge profile"))?;
        let scope = match r.byte()? {
            0 => PlayResultScope::FullSong,
            1 => {
                let start = Timestamp::from_nanos(r.i64()?);
                let end = match r.byte()? {
                    0 => None,
                    1 => Some(Timestamp::from_nanos(r.i64()?)),
                    _ => return Err(ArchiveError::Invalid("end tag")),
                };
                PlayResultScope::PracticeSection { start, end }
            }
            _ => return Err(ArchiveError::Invalid("scope tag")),
        };
        let level_units = r.u64()?;
        let failure = match r.byte()? {
            0 => None,
            1 => Some(GaugeFailure::InstantDeath),
            2 => Some(GaugeFailure::Depleted),
            _ => return Err(ArchiveError::Invalid("failure tag")),
        };
        let outcome = match r.byte()? {
            0 => PlayResultOutcome::Cleared,
            1 => PlayResultOutcome::BelowClearThreshold,
            2 => PlayResultOutcome::Failed(GaugeFailure::InstantDeath),
            3 => PlayResultOutcome::Failed(GaugeFailure::Depleted),
            _ => return Err(ArchiveError::Invalid("outcome tag")),
        };
        entries.push(ArchiveEntry {
            player,
            header: file.header,
            profile,
            result: ArchivedResult {
                scope,
                outcome,
                gauge: GaugeSnapshot {
                    level_units,
                    failure,
                },
            },
        });
    }
    if r.cursor != bytes.len() {
        return Err(ArchiveError::TrailingBytes);
    }
    let archive = ResultArchive { entries };
    validate(&archive)?;
    Ok(archive)
}

#[cfg(test)]
#[path = "result_archive_fixtures.rs"]
pub(crate) mod fixtures;

#[cfg(test)]
#[path = "result_archive_member_fixtures.rs"]
pub(crate) mod member_fixtures;
