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
    fn paused_native_mixer_prefix_resumes_equal_time_operations_without_duplicate_results() {
        use crate::replay_pause::ReplayPause;
        use beatkernel::{
            audio::{
                AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample,
                SampleBank, SampleId, VoiceId, command_queue,
            },
            time::ClockPair,
        };
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
                song_time: Timestamp::from_nanos(150_000_001),
                operation: ReplayOperation::Advance,
            },
        ];
        let full = reconstruct(&source, file.clone(), limits()).unwrap();
        let mut visual = ReplayVisual::new(&source, &file, limits()).unwrap();
        let output = |ns| ClockPoint {
            domain: ClockDomainId(1),
            timestamp: Timestamp::from_nanos(ns),
        };
        let pair = |ns| ClockPair {
            source: output(ns),
            target: ClockPoint {
                domain: ClockDomainId(2),
                timestamp: Timestamp::from_nanos(ns + 100),
            },
        };
        let format = AudioFormat::new(1000, 1).unwrap();
        let audio_limits = AudioLimits::new(8, 2, 8, 256, 8).unwrap();
        let pcm_limits = PcmLimits::new(4096, 4096, 2).unwrap();
        let mut bank = SampleBank::new(format, pcm_limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.5], pcm_limits).unwrap(),
        )
        .unwrap();
        let (mut producer, consumer) = command_queue(8).unwrap();
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::from_nanos(150_000_000),
                gain: 1.0,
            })
            .unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(format, ClockDomainId(1), Timestamp::ZERO, audio_limits),
            bank,
            consumer,
        )
        .unwrap();
        let mut pause = ReplayPause::new(
            output(0),
            ClockDomainId(2),
            1000,
            Timestamp::ZERO,
            Duration::ZERO,
        )
        .unwrap();
        let rendered = mixer.render(&mut [0.0; 149]).unwrap();
        pause.observe(Some(rendered), pair(148_000_000)).unwrap();
        assert!(
            visual
                .advance_to(
                    pause
                        .presentation_song(output(148_000_000))
                        .unwrap()
                        .unwrap()
                )
                .unwrap()
                .is_empty()
        );
        assert!(pause.request(true, pair(148_000_000)).unwrap());
        producer.request_pause(true);
        let mut silent = [1.0; 50];
        let rendered = mixer.render(&mut silent).unwrap();
        assert_eq!(silent, [0.0; 50]);
        let boundary = pause
            .observe(Some(rendered), pair(149_000_000))
            .unwrap()
            .unwrap();
        assert_eq!(boundary.song, Timestamp::from_nanos(149_000_000));
        assert!(visual.advance_to(boundary.song).unwrap().is_empty());
        let cursor = visual.cursor;
        assert!(
            pause
                .presentation_song(output(198_000_000))
                .unwrap()
                .is_none()
        );
        assert_eq!(visual.cursor, cursor);
        assert!(!visual.finished());
        assert!(pause.request(false, pair(198_000_000)).unwrap());
        producer.request_pause(false);
        let rendered = mixer.render(&mut [0.0]).unwrap();
        let boundary = pause
            .observe(Some(rendered), pair(199_000_000))
            .unwrap()
            .unwrap();
        assert!(!boundary.paused);
        assert_eq!(boundary.song, Timestamp::from_nanos(149_000_000));
        assert!(visual.advance_to(boundary.song).unwrap().is_empty());
        let mut audible = [0.0];
        mixer.render(&mut audible).unwrap();
        assert_eq!(audible, [0.5]);
        let song = pause
            .presentation_song(output(200_000_000))
            .unwrap()
            .unwrap();
        assert_eq!(song, Timestamp::from_nanos(150_000_000));
        let mut results = visual.advance_to(song).unwrap();
        assert_eq!(results.len(), 2);
        assert!(visual.advance_to(song).unwrap().is_empty());
        let song = pause
            .presentation_song(output(200_000_001))
            .unwrap()
            .unwrap();
        results.extend(visual.advance_to(song).unwrap());
        assert_eq!(results, full.results());
        assert_eq!(
            visual.engine.stable_hash().unwrap(),
            full.engine().stable_hash().unwrap()
        );
        assert!(visual.finished());
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
