use beatkernel::{
    judge::{JudgeGrade, JudgeProfile},
    time::Duration,
};
use beatkernel_bms::{
    BmsDefExRank, BmsJudgeDifficulty, BmsRank, BmsTimingPreset, BmsTimingPresetError,
    BmsTimingStage,
};

const PRESET: BmsTimingPreset = BmsTimingPreset::BeatorajaSevenKeys8320241dV1;
const STAGES: [BmsTimingStage; 4] = [
    BmsTimingStage::KeyHead,
    BmsTimingStage::ScratchHead,
    BmsTimingStage::KeyTail,
    BmsTimingStage::ScratchTail,
];

#[test]
fn every_easy_grade_matches_the_independent_pinned_microsecond_table() {
    let resolved = PRESET
        .resolve(BmsJudgeDifficulty::Rank(BmsRank::Easy))
        .unwrap();
    let expected = [
        [
            (20_000, 20_000),
            (60_000, 60_000),
            (150_000, 150_000),
            (220_000, 280_000),
        ],
        [
            (30_000, 30_000),
            (70_000, 70_000),
            (160_000, 160_000),
            (230_000, 290_000),
        ],
        [
            (120_000, 120_000),
            (160_000, 160_000),
            (200_000, 200_000),
            (220_000, 280_000),
        ],
        [
            (130_000, 130_000),
            (170_000, 170_000),
            (210_000, 210_000),
            (230_000, 290_000),
        ],
    ];
    for (stage, expected) in STAGES.into_iter().zip(expected) {
        for (entry, (early, late)) in resolved.windows(stage).iter().zip(expected) {
            assert_eq!(entry.window.early.as_nanos(), early * 1000);
            assert_eq!(entry.window.late.as_nanos(), late * 1000);
        }
    }
}

#[test]
fn rank_tables_have_distinct_head_tail_and_asymmetric_bad_bounds() {
    for (code, percentage) in [(0, 25), (1, 50), (2, 75), (3, 100), (4, 125)] {
        let difficulty = BmsJudgeDifficulty::Rank(BmsRank::parse(&code.to_string()).unwrap());
        let resolved = PRESET.resolve(difficulty).unwrap();
        assert_eq!(resolved.preset(), PRESET);
        assert_eq!(resolved.difficulty(), difficulty);
        assert_eq!(resolved.effective_percentage(), percentage);
        for (stage, pgreat, bad_early, bad_late) in [
            (BmsTimingStage::KeyHead, 20, 220, 280),
            (BmsTimingStage::ScratchHead, 30, 230, 290),
            (BmsTimingStage::KeyTail, 120, 220, 280),
            (BmsTimingStage::ScratchTail, 130, 230, 290),
        ] {
            let windows = resolved.windows(stage);
            assert_eq!(
                windows[0].window.early.as_nanos(),
                pgreat * percentage as i64 * 10_000
            );
            assert_eq!(
                windows[3].window.early.as_nanos(),
                bad_early * percentage as i64 * 10_000
            );
            assert_eq!(
                windows[3].window.late.as_nanos(),
                bad_late * percentage as i64 * 10_000
            );
        }
    }
}

#[test]
fn all_stages_are_nested_and_every_inclusive_boundary_is_exact_to_one_nanosecond() {
    let resolved = PRESET
        .resolve(BmsJudgeDifficulty::Rank(BmsRank::Normal))
        .unwrap();
    for stage in STAGES {
        let profile = JudgeProfile::new(
            resolved.windows(stage).iter().map(|w| w.window).collect(),
            Duration::ZERO,
        )
        .unwrap();
        for (index, window) in profile.windows().iter().enumerate() {
            let early = i128::from(window.early.as_nanos());
            let late = i128::from(window.late.as_nanos());
            assert_eq!(profile.grade(-early), Some(JudgeGrade(index as u32 + 1)));
            assert_eq!(profile.grade(late), Some(JudgeGrade(index as u32 + 1)));
            let next = (index < 3).then_some(JudgeGrade(index as u32 + 2));
            assert_eq!(profile.grade(-early - 1), next);
            assert_eq!(profile.grade(late + 1), next);
        }
    }
}

#[test]
fn defexrank_uses_integer_baseline_before_microsecond_scaling() {
    let difficulty = BmsJudgeDifficulty::DefExRank(BmsDefExRank::parse("101.000").unwrap());
    let resolved = PRESET.resolve(difficulty).unwrap();
    assert_eq!(resolved.effective_percentage(), 75);
    assert_eq!(
        resolved.windows(BmsTimingStage::KeyHead)[0]
            .window
            .early
            .as_nanos(),
        15_000_000
    );
    // A tiny positive source declaration scales to zero, without a fallback.
    let tiny = PRESET
        .resolve(BmsJudgeDifficulty::DefExRank(
            BmsDefExRank::parse("1").unwrap(),
        ))
        .unwrap();
    assert_eq!(tiny.effective_percentage(), 0);
    for stage in STAGES {
        assert!(tiny
            .windows(stage)
            .iter()
            .all(|w| w.window.early == Duration::ZERO && w.window.late == Duration::ZERO));
        let profile = JudgeProfile::new(
            tiny.windows(stage).iter().map(|w| w.window).collect(),
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(profile.grade(0), Some(JudgeGrade(1)));
        assert_eq!(profile.grade(-1), None);
        assert_eq!(profile.grade(1), None);
    }
}

#[test]
fn valid_metadata_can_be_unsupported_or_overflow_without_silent_fallback() {
    for (text, expected) in [
        ("0", BmsTimingPresetError::ZeroDefExRank),
        ("100.5", BmsTimingPresetError::FractionalDefExRank),
        ("999999999999999999", BmsTimingPresetError::Overflow),
    ] {
        let declaration = BmsDefExRank::parse(text).unwrap();
        assert_eq!(
            PRESET.resolve(BmsJudgeDifficulty::DefExRank(declaration)),
            Err(expected)
        );
    }
}

#[test]
fn equal_windows_still_retain_distinct_metadata_identity() {
    let rank = PRESET
        .resolve(BmsJudgeDifficulty::Rank(BmsRank::Normal))
        .unwrap();
    let defex = PRESET
        .resolve(BmsJudgeDifficulty::DefExRank(
            BmsDefExRank::parse("100").unwrap(),
        ))
        .unwrap();
    for stage in STAGES {
        assert_eq!(rank.windows(stage), defex.windows(stage));
    }
    assert_ne!(rank, defex);
    assert!(PRESET
        .id()
        .contains("8320241d8481e0826c703878c3eba01cd81ca3e4"));
}

#[test]
fn selected_source_lanes_execute_different_key_scratch_heads_and_hold_tails() {
    use beatkernel::{
        input::*,
        judge::{JudgeEngine, JudgeOutcome, JudgeStage},
        time::{ClockDomainId, ClockPoint, Timestamp},
    };
    use beatkernel_bms::{parse, BmsInputMode, ParseOptions};
    let profiles = PRESET
        .resolve(BmsJudgeDifficulty::Rank(BmsRank::Easy))
        .unwrap();
    let button = |control: u32, state| GameInputEvent {
        game_control: GameControlId(control),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(
                DeviceId(1),
                ClockPoint {
                    domain: ClockDomainId(1),
                    timestamp: Timestamp::ZERO,
                },
                1,
            ),
            control: PhysicalControlId::keyboard(control as u16),
            state,
        }),
    };
    for hold in [false, true] {
        let text = if hold {
            "#BPM 120\n#WAV01 x.wav\n#00051:0101\n#00056:0101\n"
        } else {
            "#BPM 120\n#WAV01 x.wav\n#00011:01\n#00016:01\n"
        };
        let source = parse(text, ParseOptions::default()).unwrap();
        let rules = source
            .rules_with_timing_profiles(BmsInputMode::ButtonOnly, &profiles)
            .unwrap();
        let mut engine = JudgeEngine::new(
            source.compile().unwrap().chart,
            rules,
            profiles.head_envelope(Duration::ZERO).unwrap(),
        )
        .unwrap();
        for (control, grade) in [(0x11, 2), (0x16, 1)] {
            let hits = engine
                .push_input(
                    &button(control, ButtonState::Down),
                    Timestamp::from_nanos(25_000_000),
                )
                .unwrap();
            assert_eq!(hits.len(), 1);
            assert_eq!(
                hits[0].stage,
                if hold {
                    JudgeStage::HoldHead
                } else {
                    JudgeStage::Instant
                }
            );
            assert!(
                matches!(hits[0].outcome, JudgeOutcome::Hit { grade: actual, .. } if actual == JudgeGrade(grade))
            );
        }
        if hold {
            let tail = source.compile().unwrap().chart.objects()[0]
                .time
                .end
                .unwrap()
                .as_nanos();
            for (control, grade) in [(0x11, 2), (0x16, 1)] {
                let hits = engine
                    .push_input(
                        &button(control, ButtonState::Up),
                        Timestamp::from_nanos(tail + 125_000_000),
                    )
                    .unwrap();
                assert_eq!(hits.len(), 1);
                assert_eq!(hits[0].stage, JudgeStage::HoldTail);
                assert!(
                    matches!(hits[0].outcome, JudgeOutcome::Hit { grade: actual, .. } if actual == JudgeGrade(grade))
                );
            }
        }
    }
}
