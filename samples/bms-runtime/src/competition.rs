//! Saved-record competition on the gameplay owner, never an audio callback.
//!
//! A ghost consumes only recorded operations. In particular, progressing beyond
//! a truncated file does not synthesize an advance or its timeout misses.

use crate::{
    replay_playback::{PlaybackError, reconstruct},
    timing::{TimingError, TimingSummary},
};
use beatkernel::{
    judge::{JudgeEngine, JudgeEvent, JudgeOutcome},
    replay::{
        REPLAY_VERSION, ReplayHeader, ReplayOperation,
        codec::{ReplayCodecLimits, ReplayFile},
    },
    time::Timestamp,
};
use beatkernel_bms::BmsChart;
use std::collections::BTreeMap;

/// Stage counts, with opaque grade identities and no implicit grade weighting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScoreSummary {
    /// Accepted stages, including separately emitted hold heads/tails.
    pub hits: u64,
    /// Failed stages, including recorded timeout results.
    pub misses: u64,
    /// Consecutive accepted stages since the last miss.
    pub combo: u64,
    /// Longest accepted-stage streak.
    pub max_combo: u64,
    /// Accepted-stage count per caller-defined grade identity.
    pub grades: BTreeMap<u32, u64>,
    /// Exact accepted known-stage delta statistics across the entire prefix.
    pub timing: TimingSummary,
}
impl ScoreSummary {
    /// Applies a batch atomically, rejecting counter overflow.
    pub fn observe(&mut self, events: &[JudgeEvent]) -> Result<(), CompetitionError> {
        if events.is_empty() {
            return Ok(());
        }
        let mut timing = self.timing;
        timing.observe(events)?;
        let mut hits = self.hits;
        let mut misses = self.misses;
        let mut combo = self.combo;
        let mut max_combo = self.max_combo;
        for event in events {
            match event.outcome {
                JudgeOutcome::Hit { .. } => {
                    hits = increment(hits)?;
                    combo = increment(combo)?;
                    max_combo = max_combo.max(combo);
                }
                JudgeOutcome::Miss { .. } => {
                    misses = increment(misses)?;
                    combo = 0;
                }
            }
        }
        let batch_hits = hits - self.hits;
        for (&grade, &count) in &self.grades {
            if count.checked_add(batch_hits).is_none() {
                // Only near-overflow entries need an exact borrowed batch scan.
                // An unrelated exhausted grade must not reject this observation.
                let mut next_count = count;
                for event in events {
                    if matches!(event.outcome, JudgeOutcome::Hit { grade: hit, .. } if hit.0 == grade)
                    {
                        next_count = increment(next_count)?;
                    }
                }
            }
        }
        self.hits = hits;
        self.misses = misses;
        self.combo = combo;
        self.max_combo = max_combo;
        self.timing = timing;
        for event in events {
            if let JudgeOutcome::Hit { grade, .. } = event.outcome {
                // Preflight proved every existing count fits; a new grade's
                // count is bounded by the already checked batch hit count.
                *self.grades.entry(grade.0).or_default() += 1;
            }
        }
        Ok(())
    }
}
fn increment(value: u64) -> Result<u64, CompetitionError> {
    value.checked_add(1).ok_or(CompetitionError::ScoreOverflow)
}

/// Whether a saved record is selected as the user's own or another player's.
/// This display classification is not authenticated player identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpponentKind {
    /// The user's selected past record.
    Own,
    /// A selected other player's record.
    Other,
}

/// Invalid comparison setup, bounded capacity or checked score arithmetic.
#[derive(Debug)]
pub enum CompetitionError {
    /// The logical schema is unsupported.
    UnsupportedVersion(u32),
    /// Chart, rules, seed or profile differs from the pristine local setup.
    IncompatibleSetup,
    /// The configured opponent cap has been reached.
    TooManyOpponents,
    /// A fallible vector reservation failed.
    AllocationFailed,
    /// Stage counts cannot be represented by u64.
    ScoreOverflow,
    /// Accepted timing counters or sums exceeded their scalar capacity.
    TimingOverflow,
    /// Local report time moved backward without explicit prefix rebuilding.
    TimeRegression,
    /// Canonical replay validation or builtin reconstruction failed.
    Playback(PlaybackError),
}
impl From<TimingError> for CompetitionError {
    fn from(_: TimingError) -> Self {
        Self::TimingOverflow
    }
}
impl From<PlaybackError> for CompetitionError {
    fn from(error: PlaybackError) -> Self {
        Self::Playback(error)
    }
}
impl std::fmt::Display for CompetitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BMS competition: {self:?}")
    }
}
impl std::error::Error for CompetitionError {}

/// Exact setup compatibility except for the recording's capture clock domain.
/// Identities are noncryptographic; matching is not proof of authenticity.
pub fn compatible_headers(local: &ReplayHeader, other: &ReplayHeader) -> bool {
    local.version == other.version
        && local.chart_identity == other.chart_identity
        && local.rules_identity == other.rules_identity
        && local.options == other.options
        && local.seed == other.seed
}

struct RecordedResults {
    song_time: Timestamp,
    events: Vec<JudgeEvent>,
}

/// A bounded loaded record and its displayed, recorded-operation result prefix.
pub struct GhostOpponent {
    kind: OpponentKind,
    label: String,
    batches: Vec<RecordedResults>,
    cursor: usize,
    score: ScoreSummary,
    song_time: Option<Timestamp>,
    recorded_until: Option<Timestamp>,
}
impl GhostOpponent {
    /// Caller-selected ownership category, not an authentication claim.
    pub const fn kind(&self) -> OpponentKind {
        self.kind
    }
    /// Caller-provided display label.
    pub fn label(&self) -> &str {
        &self.label
    }
    /// Actual stage summary through the current local song time.
    pub const fn score(&self) -> &ScoreSummary {
        &self.score
    }
    /// Last requested local display time; may exceed the recording's end.
    pub const fn song_time(&self) -> Option<Timestamp> {
        self.song_time
    }
    /// Last recorded operation, including operations that emitted no results.
    pub const fn recorded_until(&self) -> Option<Timestamp> {
        self.recorded_until
    }
    fn prefix(&self, time: Timestamp) -> Result<(usize, ScoreSummary), CompetitionError> {
        let cursor = self
            .batches
            .partition_point(|batch| batch.song_time <= time);
        let forward = cursor >= self.cursor;
        let mut score = if forward {
            self.score.clone()
        } else {
            ScoreSummary::default()
        };
        let start = if forward { self.cursor } else { 0 };
        for batch in &self.batches[start..cursor] {
            score.observe(&batch.events)?;
        }
        Ok((cursor, score))
    }
}

/// Local real judgments and finite saved opponents for one compiled play setup.
///
/// Construct the expected header from LiveReplayCapture on the pristine local
/// judge, before any input/advance. Loading and prefix rebuilding allocate and
/// execute only on the gameplay/control owner, outside native audio callbacks.
pub struct Competition {
    expected_header: ReplayHeader,
    max_opponents: usize,
    opponents: Vec<GhostOpponent>,
    score: ScoreSummary,
    song_time: Option<Timestamp>,
}
impl Competition {
    /// Configures the finite saved-opponent count; zero permits local scores only.
    pub fn new(
        expected_header: ReplayHeader,
        max_opponents: usize,
    ) -> Result<Self, CompetitionError> {
        if expected_header.version != REPLAY_VERSION {
            return Err(CompetitionError::UnsupportedVersion(
                expected_header.version,
            ));
        }
        Ok(Self {
            expected_header,
            max_opponents,
            opponents: Vec::new(),
            score: ScoreSummary::default(),
            song_time: None,
        })
    }
    /// Validates the complete bounded file and reconstructs through the same
    /// builtin judge as ordinary BMS replay. Failed admission leaves opponents
    /// unchanged. Operation times, rather than offset JudgeEvent.at timestamps,
    /// determine which results become visible.
    pub fn add_replay(
        &mut self,
        source: &BmsChart,
        file: ReplayFile,
        limits: ReplayCodecLimits,
        kind: OpponentKind,
        label: impl Into<String>,
    ) -> Result<usize, CompetitionError> {
        if self.opponents.len() >= self.max_opponents {
            return Err(CompetitionError::TooManyOpponents);
        }
        if !compatible_headers(&self.expected_header, &file.header) {
            return Err(CompetitionError::IncompatibleSetup);
        }
        // Canonical validation includes runtime version, actual source compilation
        // and pristine judge fingerprint, even for directly assembled ReplayFiles.
        let mut replay = reconstruct(source, file, limits)?;
        let recorded_until = replay.records().last().map(|record| record.song_time);
        replay.seek_cursor(0).map_err(PlaybackError::from)?;
        let origin = replay
            .engine()
            .snapshot()
            .map_err(beatkernel::replay::ReplayError::from)
            .map_err(PlaybackError::from)?;
        let mut judge = JudgeEngine::from_snapshot(&origin)
            .map_err(beatkernel::replay::ReplayError::from)
            .map_err(PlaybackError::from)?;
        let mut batches = Vec::new();
        let mut full_score = ScoreSummary::default();
        for record in replay.records() {
            let events = match &record.operation {
                ReplayOperation::Input(input) => judge.push_input(input, record.song_time),
                ReplayOperation::Advance => judge.advance_to(record.song_time),
            }
            .map_err(PlaybackError::from)?;
            if !events.is_empty() {
                // Validate all score arithmetic at load, including the unseen tail.
                full_score.observe(&events)?;
                batches
                    .try_reserve(1)
                    .map_err(|_| CompetitionError::AllocationFailed)?;
                batches.push(RecordedResults {
                    song_time: record.song_time,
                    events,
                });
            }
        }
        let mut opponent = GhostOpponent {
            kind,
            label: label.into(),
            batches,
            cursor: 0,
            score: ScoreSummary::default(),
            song_time: self.song_time,
            recorded_until,
        };
        if let Some(time) = self.song_time {
            let (cursor, score) = opponent.prefix(time)?;
            opponent.cursor = cursor;
            opponent.score = score;
        }
        self.opponents
            .try_reserve(1)
            .map_err(|_| CompetitionError::AllocationFailed)?;
        let index = self.opponents.len();
        self.opponents.push(opponent);
        Ok(index)
    }
    /// Consumes each actual RuntimeReport's stage results exactly once, in report
    /// order. Empty batches still progress ghosts. A backward restart requires
    /// rebuild with the actual local result prefix, or reset for a fresh session.
    pub fn observe(
        &mut self,
        events: &[JudgeEvent],
        song_time: Timestamp,
    ) -> Result<(), CompetitionError> {
        if self.song_time.is_some_and(|previous| song_time < previous) {
            return Err(CompetitionError::TimeRegression);
        }
        if self.opponents.is_empty() {
            self.score.observe(events)?;
            self.song_time = Some(song_time);
            return Ok(());
        }
        let mut score = self.score.clone();
        score.observe(events)?;
        self.update(score, song_time)
    }
    /// Replaces the local summary with an actual reconstructed result prefix and
    /// seeks saved opponents through recorded operations only, in either direction.
    pub fn rebuild(
        &mut self,
        local_prefix: &[JudgeEvent],
        song_time: Timestamp,
    ) -> Result<(), CompetitionError> {
        let mut score = ScoreSummary::default();
        score.observe(local_prefix)?;
        self.update(score, song_time)
    }
    fn update(&mut self, score: ScoreSummary, time: Timestamp) -> Result<(), CompetitionError> {
        // Prepare all updates before committing either local or remote display state.
        let mut updates = Vec::new();
        updates
            .try_reserve_exact(self.opponents.len())
            .map_err(|_| CompetitionError::AllocationFailed)?;
        for opponent in &self.opponents {
            updates.push(opponent.prefix(time)?);
        }
        for (opponent, (cursor, score)) in self.opponents.iter_mut().zip(updates) {
            opponent.cursor = cursor;
            opponent.score = score;
            opponent.song_time = Some(time);
        }
        self.score = score;
        self.song_time = Some(time);
        Ok(())
    }
    /// Clears displayed scores/progress, retaining loaded validated recordings.
    pub fn reset(&mut self) {
        self.score = ScoreSummary::default();
        self.song_time = None;
        for opponent in &mut self.opponents {
            opponent.cursor = 0;
            opponent.score = ScoreSummary::default();
            opponent.song_time = None;
        }
    }
    /// Pristine local chart/rules/profile identity used for comparison.
    pub const fn expected_header(&self) -> &ReplayHeader {
        &self.expected_header
    }
    /// Actual local stage summary.
    pub const fn score(&self) -> &ScoreSummary {
        &self.score
    }
    /// Unused capacity under this comparison's configured opponent bound.
    pub fn remaining_opponent_capacity(&self) -> usize {
        self.max_opponents - self.opponents.len()
    }

    /// Loaded saved opponents and their current recorded prefixes.
    pub fn opponents(&self) -> &[GhostOpponent] {
        &self.opponents
    }
    /// Last accepted local report or explicit rebuild time.
    pub const fn song_time(&self) -> Option<Timestamp> {
        self.song_time
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay_capture::LiveReplayCapture;
    use beatkernel::{
        chart::ObjectId,
        input::{
            ButtonEvent, ButtonState, CodecLimits, DeviceId, EventMeta, GameInputEvent,
            PhysicalControlId, PhysicalInputEvent,
        },
        judge::{JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow, MissReason},
        replay::ReplaySession,
        time::{ClockDomainId, ClockPoint, Duration},
    };
    use beatkernel_bms::{ParseOptions, parse};

    fn ts(nanos: i64) -> Timestamp {
        Timestamp::from_nanos(nanos)
    }
    fn limits() -> ReplayCodecLimits {
        ReplayCodecLimits::new(1 << 20, 100, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
    }
    fn fixture(domain: ClockDomainId, complete: bool) -> (BmsChart, ReplayFile, Vec<JudgeEvent>) {
        let source = parse(
            "#BPM 60\n#WAV01 tap.wav\n#00011:00010001\n",
            ParseOptions::default(),
        )
        .unwrap();
        let judge = JudgeEngine::new(
            source.compile().unwrap().chart,
            source.rules(),
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(7),
                    early: Duration::ZERO,
                    late: Duration::ZERO,
                }],
                Duration::from_nanos(10),
            )
            .unwrap(),
        )
        .unwrap();
        let header = LiveReplayCapture::new(&judge, domain, limits())
            .unwrap()
            .header()
            .clone();
        let mut session = ReplaySession::new(header.clone(), judge).unwrap();
        let control = source.notes[0].lane.control();
        let input = GameInputEvent {
            game_control: control,
            physical: PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(
                    DeviceId(3),
                    ClockPoint {
                        domain,
                        timestamp: ts(99),
                    },
                    0,
                ),
                control: PhysicalControlId::keyboard(4),
                state: ButtonState::Down,
            }),
        };
        session.push_input(input, ts(999_999_990)).unwrap();
        if complete {
            session.advance_to(ts(3_000_000_000)).unwrap();
        }
        let events = session.results().to_vec();
        (
            source,
            ReplayFile::new(header, session.records().to_vec()),
            events,
        )
    }

    #[test]
    fn different_capture_domain_is_compatible_and_operation_time_controls_visibility() {
        let (_, local, _) = fixture(ClockDomainId(1), false);
        let (source, remote, events) = fixture(ClockDomainId(2), false);
        assert_eq!(events[0].at, ts(1_000_000_000));
        let mut competition = Competition::new(local.header, 1).unwrap();
        competition
            .add_replay(&source, remote, limits(), OpponentKind::Other, "other")
            .unwrap();
        competition.observe(&[], ts(999_999_989)).unwrap();
        assert_eq!(competition.opponents()[0].score().hits, 0);
        competition.observe(&events, ts(999_999_990)).unwrap();
        assert_eq!(competition.opponents()[0].score().hits, 1);
        assert_eq!(competition.score(), competition.opponents()[0].score());
        assert_eq!(competition.opponents()[0].kind(), OpponentKind::Other);
    }

    #[test]
    fn truncated_record_never_fabricates_future_misses_and_backward_rebuild_is_exact() {
        let (source, file, events) = fixture(ClockDomainId(2), false);
        let mut competition = Competition::new(file.header.clone(), 1).unwrap();
        competition
            .add_replay(&source, file, limits(), OpponentKind::Own, "past")
            .unwrap();
        competition.observe(&events, ts(999_999_990)).unwrap();
        competition.observe(&[], ts(20_000_000_000)).unwrap();
        assert_eq!(competition.opponents()[0].score().hits, 1);
        assert_eq!(competition.opponents()[0].score().misses, 0);
        assert_eq!(
            competition.opponents()[0].recorded_until(),
            Some(ts(999_999_990))
        );
        assert!(matches!(
            competition.observe(&[], ts(0)),
            Err(CompetitionError::TimeRegression)
        ));
        competition.rebuild(&[], ts(999_999_989)).unwrap();
        assert_eq!(competition.score(), &ScoreSummary::default());
        assert_eq!(competition.opponents()[0].score(), &ScoreSummary::default());
        competition.rebuild(&events, ts(999_999_990)).unwrap();
        assert_eq!(competition.opponents()[0].score().hits, 1);
        competition.reset();
        assert_eq!(competition.song_time(), None);
        assert_eq!(competition.opponents()[0].song_time(), None);
        competition.observe(&[], ts(999_999_990)).unwrap();
        assert_eq!(competition.opponents()[0].score().hits, 1);
        assert_eq!(competition.score().hits, 0);
    }

    #[test]
    fn recorded_misses_advance_once_and_late_loaded_opponent_starts_at_current_prefix() {
        let (source, file, events) = fixture(ClockDomainId(2), true);
        assert_eq!(events.len(), 2);
        let mut competition = Competition::new(file.header.clone(), 1).unwrap();
        competition.observe(&events, ts(3_000_000_000)).unwrap();
        competition
            .add_replay(&source, file, limits(), OpponentKind::Own, "own")
            .unwrap();
        assert_eq!(competition.opponents()[0].score(), competition.score());
        competition.observe(&[], ts(3_000_000_000)).unwrap();
        competition.observe(&[], ts(4_000_000_000)).unwrap();
        assert_eq!(competition.opponents()[0].score().hits, 1);
        assert_eq!(competition.opponents()[0].score().misses, 1);
        assert_eq!(competition.opponents()[0].score().combo, 0);
        assert_eq!(competition.opponents()[0].score().max_combo, 1);
    }

    #[test]
    fn setup_runtime_and_capacity_rejections_leave_opponents_unchanged() {
        let (source, file, _) = fixture(ClockDomainId(2), false);
        let mut competition = Competition::new(file.header.clone(), 1).unwrap();
        for changed in 0..5 {
            let mut incompatible = file.clone();
            match changed {
                0 => incompatible.header.chart_identity.push(0),
                1 => incompatible.header.rules_identity.push(0),
                2 => incompatible.header.options.push(0),
                3 => incompatible.header.seed += 1,
                _ => incompatible.header.version += 1,
            }
            assert!(matches!(
                competition.add_replay(&source, incompatible, limits(), OpponentKind::Other, "bad"),
                Err(CompetitionError::IncompatibleSetup)
            ));
            assert!(competition.opponents().is_empty());
        }
        let mut version = file.clone();
        version.runtime_version.push_str("-different");
        assert!(matches!(
            competition.add_replay(&source, version, limits(), OpponentKind::Own, "bad"),
            Err(CompetitionError::Playback(PlaybackError::IdentityMismatch(
                "runtime version"
            )))
        ));
        assert!(competition.opponents().is_empty());
        competition
            .add_replay(&source, file.clone(), limits(), OpponentKind::Own, "first")
            .unwrap();
        assert!(matches!(
            competition.add_replay(&source, file, limits(), OpponentKind::Other, "second"),
            Err(CompetitionError::TooManyOpponents)
        ));
        assert_eq!(competition.opponents().len(), 1);
    }

    #[test]
    fn pristine_identity_is_checked_against_actual_source_not_only_supplied_header() {
        let (source, mut file, _) = fixture(ClockDomainId(2), false);
        file.header.chart_identity.push(99);
        let mut competition = Competition::new(file.header.clone(), 1).unwrap();
        assert!(matches!(
            competition.add_replay(&source, file, limits(), OpponentKind::Own, "forged"),
            Err(CompetitionError::Playback(PlaybackError::IdentityMismatch(
                _
            )))
        ));
        assert!(competition.opponents().is_empty());
    }

    #[test]
    fn each_stage_counts_in_order_and_overflow_is_atomic() {
        let event = |stage, outcome| JudgeEvent {
            object: ObjectId(1),
            stage,
            outcome,
            at: ts(10),
            input: None,
        };
        let hit = JudgeOutcome::Hit {
            grade: JudgeGrade(7),
            delta: Duration::ZERO,
        };
        let miss = JudgeOutcome::Miss {
            reason: MissReason::TailTimeout,
        };
        let mut score = ScoreSummary::default();
        score
            .observe(&[
                event(JudgeStage::HoldHead, hit),
                event(JudgeStage::HoldTail, hit),
                event(JudgeStage::Instant, miss),
                event(JudgeStage::Custom(9), hit),
            ])
            .unwrap();
        assert_eq!(
            (score.hits, score.misses, score.combo, score.max_combo),
            (3, 1, 1, 2)
        );
        assert_eq!(score.grades.get(&7), Some(&3));
        for field in 0..4 {
            let mut exhausted = score.clone();
            match field {
                0 => exhausted.hits = u64::MAX,
                1 => exhausted.combo = u64::MAX,
                2 => {
                    exhausted.grades.insert(7, u64::MAX);
                }
                _ => exhausted.misses = u64::MAX,
            }
            let before = exhausted.clone();
            let outcome = if field == 3 { miss } else { hit };
            assert!(matches!(
                exhausted.observe(&[event(JudgeStage::Instant, outcome)]),
                Err(CompetitionError::ScoreOverflow)
            ));
            assert_eq!(exhausted, before);
        }
    }
    #[test]
    fn full_timing_prefix_exceeds_history_cap_and_matches_saved_rebuild() {
        let event = |index: usize| JudgeEvent {
            object: ObjectId(index as u64 + 1),
            stage: JudgeStage::Instant,
            outcome: JudgeOutcome::Hit {
                grade: JudgeGrade(42),
                delta: Duration::from_nanos(if index % 2 == 0 { -3 } else { 5 }),
            },
            at: ts(index as i64),
            input: None,
        };
        let events: Vec<_> = (0..257).map(event).collect();
        let mut all = ScoreSummary::default();
        all.observe(&events).unwrap();
        assert_eq!(all.timing.count(), 257);
        assert_eq!(all.timing.early(), 129);
        assert_eq!(all.timing.late(), 128);
        assert_eq!(all.timing.mean_ns(), Some(0));
        assert_eq!(all.timing.mean_absolute_ns(), Some(3));
        let mut chunks = ScoreSummary::default();
        for part in events.chunks(17) {
            chunks.observe(part).unwrap();
        }
        assert_eq!(chunks, all);
        let (source, file, actual) = fixture(ClockDomainId(3), true);
        let mut saved = Competition::new(file.header.clone(), 1).unwrap();
        saved
            .add_replay(&source, file, limits(), OpponentKind::Own, "timing")
            .unwrap();
        saved.observe(&actual, ts(3_000_000_000)).unwrap();
        assert_eq!(saved.score().timing, saved.opponents()[0].score().timing);
        assert_eq!(saved.score().timing.count(), 1);
        assert_eq!(saved.score().timing.exact(), 1);
        saved.rebuild(&[], ts(0)).unwrap();
        assert_eq!(saved.score().timing, TimingSummary::default());
        saved.rebuild(&actual, ts(3_000_000_000)).unwrap();
        assert_eq!(saved.score().timing, saved.opponents()[0].score().timing);
    }
    #[test]
    fn score_and_timing_overflow_preserve_each_other_atomically() {
        let hit = JudgeEvent {
            object: ObjectId(1),
            stage: JudgeStage::Instant,
            outcome: JudgeOutcome::Hit {
                grade: JudgeGrade(7),
                delta: Duration::from_nanos(-2),
            },
            at: Timestamp::ZERO,
            input: None,
        };
        let mut score = ScoreSummary::default();
        score.observe(&[hit]).unwrap();
        score.hits = u64::MAX;
        let before = score.clone();
        assert!(matches!(
            score.observe(&[hit]),
            Err(CompetitionError::ScoreOverflow)
        ));
        assert_eq!(score, before);
        let mut exhausted = ScoreSummary::default();
        exhausted.timing = TimingSummary::exhausted_for_fixture();
        let before = exhausted.clone();
        assert!(matches!(
            exhausted.observe(&[hit]),
            Err(CompetitionError::TimingOverflow)
        ));
        assert_eq!(exhausted, before);
        let mut custom = hit;
        custom.stage = JudgeStage::Custom(123);
        exhausted.observe(&[custom]).unwrap();
        assert_eq!(exhausted.hits, 1);
        assert_eq!(exhausted.timing, before.timing);
    }
}

#[cfg(test)]
#[path = "score_observation_fixtures.rs"]
mod score_observation_fixtures;
