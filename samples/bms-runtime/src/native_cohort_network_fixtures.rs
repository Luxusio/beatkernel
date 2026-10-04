//! Deferred cohort snapshot/finalization policy; transport and devices stay absent.
use super::*;
use crate::{
    competition_live::replay_limits,
    local_runtime::{FailureKind, InputResult},
    multiplayer::Progress,
    multiplayer_group::MemberProgress,
    native_cohort::member_progress,
};
use beatkernel::{
    audio::{AudioCommand, SampleId, command_queue},
    input::{ButtonEvent, ButtonState, EventMeta, PhysicalInputEvent},
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow},
    replay::codec::{ReplayFile, encode_replay},
    runtime::{RuntimeProcessingClock, SoundBinding},
    time::{ClockMapper, ClockMappingQuality, Duration},
    transport::Rate,
};
use beatkernel_bms::{ParseOptions, parse_seeded};

fn states(count: usize) -> Vec<PlayerState> {
    (0..count)
        .map(|index| PlayerState {
            player: PlayerId(u32::MAX - index as u32),
            capture: None,
            competition: None,
            completion: None,
            score: ScoreSummary::default(),
            last_song: Timestamp::from_nanos(-100_000_000),
        })
        .collect()
}

fn retained(states: &[PlayerState]) -> Vec<(PlayerId, Timestamp, ScoreSummary)> {
    states
        .iter()
        .map(|state| (state.player, state.last_song, state.score.clone()))
        .collect()
}

#[test]
fn assigned_cohort_count_admission_is_identical_with_or_without_network() {
    for count in [2, 3, 4, 64] {
        assert!(admit_cohort(count, false).is_ok());
        assert!(admit_cohort(count, true).is_ok());
    }
    for count in [0, 1, 65, usize::MAX] {
        assert!(admit_cohort(count, false).is_err());
        assert!(admit_cohort(count, true).is_err());
    }
}

#[test]
fn whole_retained_prefix_keeps_original_order_signed_frontiers_and_full_width_counters() {
    for count in [2, 3, 4, 64] {
        let mut states = states(count);
        states[0].last_song = Timestamp::from_nanos(i64::MIN);
        states[0].score.hits = u64::MAX;
        states[0].score.combo = u64::MAX;
        states[0].score.max_combo = u64::MAX;
        states[1].last_song = Timestamp::from_nanos(i64::MAX);
        states[1].score.misses = u64::MAX;
        for (index, state) in states.iter_mut().enumerate().skip(2) {
            state.last_song = Timestamp::from_nanos(604_800_000_000_001 + index as i64);
            state.score.hits = index as u64;
            state.score.combo = index as u64;
            state.score.max_combo = index as u64;
        }
        let before = retained(&states);
        let snapshot = member_progress(&states).unwrap();
        assert_eq!(snapshot.len(), count);
        assert_eq!(
            snapshot[0],
            MemberProgress {
                player: PlayerId(u32::MAX),
                progress: Progress {
                    song_ns: i64::MIN,
                    hits: u64::MAX,
                    misses: 0,
                    combo: u64::MAX,
                    max_combo: u64::MAX
                },
            }
        );
        assert_eq!(
            snapshot[1],
            MemberProgress {
                player: PlayerId(u32::MAX - 1),
                progress: Progress {
                    song_ns: i64::MAX,
                    hits: 0,
                    misses: u64::MAX,
                    combo: 0,
                    max_combo: 0
                },
            }
        );
        assert_eq!(
            snapshot
                .iter()
                .map(|member| member.player)
                .collect::<Vec<_>>(),
            states.iter().map(|state| state.player).collect::<Vec<_>>()
        );
        assert_eq!(retained(&states), before);
        states[0].score = ScoreSummary::default();
        states[0].last_song = Timestamp::ZERO;
        assert_eq!(snapshot[0].progress.hits, u64::MAX);
        assert_eq!(snapshot[0].progress.song_ns, i64::MIN);
    }
}

#[test]
fn invalid_later_member_refuses_the_entire_snapshot_without_rewriting_retained_state() {
    for case in 0..5 {
        let mut states = states(4);
        states[0].score.hits = 1;
        states[0].score.combo = 1;
        states[0].score.max_combo = 1;
        match case {
            0 => states[3].player = PlayerId(0),
            1 => states[3].player = states[0].player,
            2 => states[3].score.combo = 1,
            3 => states[3].score.max_combo = 1,
            _ => {
                states[3].score.hits = u64::MAX;
                states[3].score.misses = 1;
            }
        }
        let before = retained(&states);
        assert!(member_progress(&states).is_err());
        assert_eq!(retained(&states), before);
    }
    assert!(member_progress(&[]).is_err());
    assert!(member_progress(&states(65)).is_err());
}

struct SameDomain;
impl ClockMapper for SameDomain {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}

#[test]
fn finalization_receives_actual_partial_runtime_prefix_once_and_keeps_capture_and_prior_errors() {
    let source = parse_seeded(
        "#BPM 60\n#WAV01 key.wav\n#00011:01",
        ParseOptions::default(),
        0,
    )
    .unwrap();
    let host = ClockDomainId(17);
    let output = ClockDomainId(18);
    let origin = Timestamp::from_nanos(604_800_000_000_017);
    let at = ClockPoint {
        domain: host,
        timestamp: origin,
    };
    let audio_at = ClockPoint {
        domain: output,
        timestamp: Timestamp::from_nanos(23),
    };
    let limits = replay_limits().unwrap();
    let mut states = states(3);
    let mut configs = Vec::new();
    for (index, state) in states.iter_mut().enumerate() {
        let judge = JudgeEngine::new(
            source.compile().unwrap().chart,
            source.rules(),
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(1),
                    early: Duration::ZERO,
                    late: Duration::ZERO,
                }],
                Duration::ZERO,
            )
            .unwrap(),
        )
        .unwrap();
        state.capture = Some(LiveReplayCapture::new(&judge, host, limits).unwrap());
        let device = DeviceId(u64::MAX - index as u64);
        configs.push(MemberConfig {
            player: state.player,
            device: Some(device),
            bindings: BindingMap::from_bindings([Binding {
                device: DeviceSelector::Exact(device),
                physical: PhysicalControlId::keyboard(4),
                game_control: GameControlId(0x11),
            }])
            .unwrap(),
            judge,
            sounds: vec![SoundBinding {
                object: source.notes[0].object,
                stage: JudgeStage::Instant,
                sample: SampleId(1),
                voice: VoiceId(index as u64 + 1),
                gain: 1.0,
            }],
        });
    }
    let (producer, mut consumer) = command_queue(1).unwrap();
    let mut group = RuntimeGroup::new(
        host,
        output,
        Transport::new(origin, Timestamp::ZERO, Rate::NORMAL),
        producer,
        configs,
        0,
        &[],
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    let input = |device| {
        PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(device), at, u64::MAX),
            control: PhysicalControlId::keyboard(4),
            state: ButtonState::Down,
        })
    };
    let mut reports = match group
        .process_input(input(u64::MAX), &SameDomain, audio_at)
        .unwrap()
    {
        InputResult::Processed(reports) => reports,
        InputResult::Ignored { .. } => panic!("assigned source was ignored"),
    };
    let error = group
        .process_input(input(u64::MAX - 1), &SameDomain, audio_at)
        .unwrap_err();
    assert!(matches!(error.kind, FailureKind::ReportedFailure));
    assert_eq!(error.failed_player, Some(states[1].player));
    assert_eq!(error.completed_reports.len(), 1);
    assert_eq!(error.completed_reports[0].report.audio_failures.len(), 1);
    assert!(group.poisoned());
    reports.extend(error.completed_reports);
    for tagged in &reports {
        let state = states
            .iter_mut()
            .find(|state| state.player == tagged.player)
            .unwrap();
        state.last_song = tagged.report.song_time;
        state.score.observe(&tagged.report.judge_events).unwrap();
        state
            .capture
            .as_mut()
            .unwrap()
            .record_report(&tagged.report)
            .unwrap();
    }
    let expected = vec![
        MemberProgress {
            player: states[0].player,
            progress: Progress {
                song_ns: 0,
                hits: 1,
                misses: 0,
                combo: 1,
                max_combo: 1,
            },
        },
        MemberProgress {
            player: states[1].player,
            progress: Progress {
                song_ns: 0,
                hits: 1,
                misses: 0,
                combo: 1,
                max_combo: 1,
            },
        },
        MemberProgress {
            player: states[2].player,
            progress: Progress {
                song_ns: -100_000_000,
                hits: 0,
                misses: 0,
                combo: 0,
                max_combo: 0,
            },
        },
    ];
    assert_eq!(member_progress(&states).unwrap(), expected);
    assert_eq!(
        consumer.try_pop().unwrap(),
        AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: audio_at.timestamp,
            gain: 1.0,
        }
    );
    let captures = states
        .iter()
        .map(|state| {
            let capture = state.capture.as_ref().unwrap();
            encode_replay(
                &ReplayFile::new(capture.header().clone(), capture.records().to_vec()),
                limits,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(states[0].capture.as_ref().unwrap().records().len(), 1);
    assert_eq!(states[1].capture.as_ref().unwrap().records().len(), 1);
    assert!(states[2].capture.as_ref().unwrap().records().is_empty());
    let hashes = states
        .iter()
        .map(|state| {
            group
                .member_judge(state.player)
                .unwrap()
                .stable_hash()
                .unwrap()
        })
        .collect::<Vec<_>>();
    for cleanup_fails in [false, true] {
        let mut failures = vec!["native output cleanup failed earlier".to_string()];
        let mut calls = 0;
        finalize_network(&states, &mut failures, |members| {
            calls += 1;
            assert_eq!(members, expected.as_slice());
            if cleanup_fails {
                Err("shared worker join failed".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(calls, 1);
        assert_eq!(failures.len(), if cleanup_fails { 2 } else { 1 });
        assert_eq!(failures[0], "native output cleanup failed earlier");
        if cleanup_fails {
            assert!(failures[1].contains("shared worker join failed"));
        }
    }
    assert_eq!(member_progress(&states).unwrap(), expected);
    assert_eq!(
        states
            .iter()
            .map(|state| group
                .member_judge(state.player)
                .unwrap()
                .stable_hash()
                .unwrap())
            .collect::<Vec<_>>(),
        hashes
    );
    for (state, bytes) in states.into_iter().zip(captures) {
        assert_eq!(state.capture.unwrap().into_bytes().unwrap(), bytes);
    }
}

#[test]
fn invalid_final_snapshot_still_invokes_cleanup_once_and_absent_owner_is_no_action() {
    let mut states = states(3);
    states[2].score.combo = 1;
    let snapshot_error = member_progress(&states).unwrap_err().to_string();
    let before = retained(&states);
    for cleanup_fails in [false, true] {
        let mut failures = vec!["original input cleanup failure".to_string()];
        let mut calls = 0;
        finalize_network(&states, &mut failures, |members| {
            calls += 1;
            assert!(members.is_empty());
            if cleanup_fails {
                Err("join failure after invalid snapshot".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(calls, 1);
        assert_eq!(failures.len(), if cleanup_fails { 3 } else { 2 });
        assert_eq!(failures[0], "original input cleanup failure");
        assert!(failures[1].contains(&snapshot_error));
        if cleanup_fails {
            assert!(failures[2].contains("join failure after invalid snapshot"));
        }
    }
    assert_eq!(retained(&states), before);
    let mut failures = vec!["setup was cancelled before an owner existed".to_string()];
    let original = failures.clone();
    finish_cohort_network(None, &states, &mut failures);
    finish_cohort_network(None, &[], &mut failures);
    assert_eq!(failures, original);
    assert_eq!(retained(&states), before);
}
