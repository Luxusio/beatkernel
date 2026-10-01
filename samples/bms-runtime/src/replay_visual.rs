//! Incremental actual recorded operations for presentation, without synthetic judging.
use crate::replay_playback::{decode_setup, reconstruct, validate_setup};
use beatkernel::{
    judge::{JudgeEngine, JudgeEvent},
    replay::{
        ReplayOperation, ReplayRecord,
        codec::{ReplayCodecLimits, ReplayFile},
    },
    time::Timestamp,
};
use beatkernel_bms::BmsChart;
type BoxError = Box<dyn std::error::Error>;

/// A fully validated log with an independent pristine judge and forward cursor.
/// Presentation targets may precede the first operation (including preroll).
/// Only operations in the retained log can produce results; its end is a prefix.
pub struct ReplayVisual {
    engine: JudgeEngine,
    records: Vec<ReplayRecord>,
    cursor: usize,
    start: Timestamp,
    observed: Option<Timestamp>,
}
impl ReplayVisual {
    /// Validates codec bounds and setup before cloning, and every operation before use.
    pub fn new(
        source: &BmsChart,
        file: &ReplayFile,
        limits: ReplayCodecLimits,
    ) -> Result<Self, BoxError> {
        let engine = validate_setup(source, file, limits)?;
        // Whole-log judge validation must precede native output. The clone is
        // bounded by canonical validation above, not trusted caller extents.
        drop(reconstruct(source, file.clone(), limits)?);
        let (_, start) = decode_setup(&file.header.options)?;
        Ok(Self {
            engine,
            records: file.records.clone(),
            cursor: 0,
            start,
            observed: None,
        })
    }
    pub const fn start(&self) -> Timestamp {
        self.start
    }
    pub fn recorded_until(&self) -> Option<Timestamp> {
        self.records.last().map(|record| record.song_time)
    }
    /// Applies every actual operation at or before this presentation target.
    /// Equal-time operations retain their validated ordinal order. Regressions
    /// reject before mutation, including after the actual prefix has finished.
    pub fn advance_to(&mut self, song: Timestamp) -> Result<Vec<JudgeEvent>, BoxError> {
        if self.observed.is_some_and(|prior| song < prior) {
            return Err("replay presentation target regressed".into());
        }
        let mut results = Vec::new();
        while let Some(record) = self
            .records
            .get(self.cursor)
            .filter(|record| record.song_time <= song)
        {
            let events = match &record.operation {
                ReplayOperation::Input(input) => self.engine.push_input(input, record.song_time)?,
                ReplayOperation::Advance => self.engine.advance_to(record.song_time)?,
            };
            results.extend(events);
            self.cursor += 1;
        }
        self.observed = Some(song);
        Ok(results)
    }
    pub fn finished(&self) -> bool {
        self.cursor == self.records.len()
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use crate::replay_capture::LiveReplayCapture;
    use beatkernel::{
        input::{
            ButtonEvent, ButtonState, CodecLimits, DeviceId, EventMeta, GameControlId,
            GameInputEvent, PhysicalControlId, PhysicalInputEvent,
        },
        judge::{JudgeGrade, JudgeOutcome, JudgeProfile, JudgeWindow},
        replay::ReplayOperation,
        time::{ClockDomainId, ClockPoint, Duration},
    };
    use beatkernel_bms::{ParseOptions, parse};
    fn limits() -> ReplayCodecLimits {
        ReplayCodecLimits::new(8192, 16, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
    }
    fn source() -> BmsChart {
        parse(
            "#BPM 120\n#00011:01\n#00012:01\n#00014:01\n#00113:01\n",
            ParseOptions::default(),
        )
        .unwrap()
    }
    fn file() -> ReplayFile {
        let source = source();
        let judge = JudgeEngine::new(
            source.compile().unwrap().chart,
            source.rules(),
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(1),
                    early: Duration::from_nanos(150_000_000),
                    late: Duration::from_nanos(150_000_000),
                }],
                Duration::ZERO,
            )
            .unwrap(),
        )
        .unwrap();
        LiveReplayCapture::new(&judge, ClockDomainId(17), limits())
            .unwrap()
            .into_file()
    }
    fn input(lane: u32, sequence: u64) -> ReplayOperation {
        ReplayOperation::Input(GameInputEvent {
            game_control: GameControlId(lane),
            physical: PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(
                    DeviceId(1),
                    ClockPoint {
                        domain: ClockDomainId(17),
                        timestamp: Timestamp::from_nanos(150_000_000),
                    },
                    sequence,
                ),
                control: PhysicalControlId::keyboard(4),
                state: ButtonState::Down,
            }),
        })
    }
    #[test]
    fn incremental_equal_time_inputs_inclusive_edge_and_prefix_match_full_validation() {
        let source = source();
        let mut file = file();
        file.records = vec![
            ReplayRecord {
                ordinal: 0,
                song_time: Timestamp::from_nanos(150_000_000),
                operation: input(0x11, 1),
            },
            ReplayRecord {
                ordinal: 1,
                song_time: Timestamp::from_nanos(150_000_000),
                operation: input(0x12, 2),
            },
            ReplayRecord {
                ordinal: 2,
                song_time: Timestamp::from_nanos(150_000_000),
                operation: ReplayOperation::Advance,
            },
            ReplayRecord {
                ordinal: 3,
                song_time: Timestamp::from_nanos(150_000_001),
                operation: ReplayOperation::Advance,
            },
        ];
        let full = reconstruct(&source, file.clone(), limits()).unwrap();
        let mut visual = ReplayVisual::new(&source, &file, limits()).unwrap();
        assert_eq!(visual.start(), Timestamp::ZERO);
        assert_eq!(
            visual.recorded_until(),
            Some(Timestamp::from_nanos(150_000_001))
        );
        assert!(
            visual
                .advance_to(Timestamp::from_nanos(-3_000_000_000))
                .unwrap()
                .is_empty()
        );
        assert!(
            visual
                .advance_to(Timestamp::from_nanos(149_999_999))
                .unwrap()
                .is_empty()
        );
        let mut events = visual
            .advance_to(Timestamp::from_nanos(150_000_000))
            .unwrap();
        assert_eq!(events.len(), 2);
        assert!(
            events
                .iter()
                .all(|event| matches!(event.outcome, JudgeOutcome::Hit { .. }))
        );
        assert!(!visual.finished());
        assert!(
            visual
                .advance_to(Timestamp::from_nanos(150_000_000))
                .unwrap()
                .is_empty()
        );
        let misses = visual
            .advance_to(Timestamp::from_nanos(150_000_001))
            .unwrap();
        assert_eq!(misses.len(), 1);
        assert!(matches!(misses[0].outcome, JudgeOutcome::Miss { .. }));
        events.extend(misses);
        assert_eq!(events, full.results());
        assert!(visual.finished());
        let cursor = visual.cursor;
        assert!(visual.advance_to(Timestamp::ZERO).is_err());
        assert_eq!(visual.cursor, cursor);
        assert!(
            visual
                .advance_to(Timestamp::from_nanos(i64::MAX))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            visual.engine.effective_song_time(),
            full.engine().effective_song_time()
        );
    }
    #[test]
    fn empty_is_finished_and_validation_rejects_invalid_whole_log_before_use() {
        let source = source();
        let file = file();
        let mut visual = ReplayVisual::new(&source, &file, limits()).unwrap();
        assert!(visual.finished());
        assert_eq!(visual.recorded_until(), None);
        assert!(
            visual
                .advance_to(Timestamp::from_nanos(-1))
                .unwrap()
                .is_empty()
        );
        assert!(
            visual
                .advance_to(Timestamp::from_nanos(i64::MAX))
                .unwrap()
                .is_empty()
        );
        assert_eq!(visual.engine.effective_song_time(), None);
        let mut invalid = file.clone();
        invalid.records.push(ReplayRecord {
            ordinal: 1,
            song_time: Timestamp::ZERO,
            operation: ReplayOperation::Advance,
        });
        assert!(ReplayVisual::new(&source, &invalid, limits()).is_err());
        let mut oversized = file.clone();
        oversized.header.options.resize(4097, 0);
        assert!(ReplayVisual::new(&source, &oversized, limits()).is_err());
        let mut invalid = file;
        invalid.records.push(ReplayRecord {
            ordinal: 0,
            song_time: Timestamp::ZERO,
            operation: input(0x11, 1),
        });
        // Native input timestamp is150ms but song operation is0: same-domain
        // metadata is retained; a replay operation may use a mapped song time.
        invalid.records.push(ReplayRecord {
            ordinal: 1,
            song_time: Timestamp::from_nanos(-1),
            operation: ReplayOperation::Advance,
        });
        assert!(ReplayVisual::new(&source, &invalid, limits()).is_err());
        // Canonical order and header bounds alone cannot prove operations valid:
        // this actual advance overflows the profile's effective song timestamp.
        let overflow_judge = JudgeEngine::new(
            source.compile().unwrap().chart,
            source.rules(),
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(1),
                    early: Duration::ZERO,
                    late: Duration::ZERO,
                }],
                Duration::from_nanos(i64::MAX),
            )
            .unwrap(),
        )
        .unwrap();
        let mut invalid = LiveReplayCapture::new(&overflow_judge, ClockDomainId(17), limits())
            .unwrap()
            .into_file();
        invalid.records.push(ReplayRecord {
            ordinal: 0,
            song_time: Timestamp::from_nanos(1),
            operation: ReplayOperation::Advance,
        });
        assert!(ReplayVisual::new(&source, &invalid, limits()).is_err());
    }
}
