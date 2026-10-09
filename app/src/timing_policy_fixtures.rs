use super::*;
use beatkernel::{
    input::*,
    judge::{JudgeOutcome, JudgeStage},
    time::{ClockDomainId, ClockPoint, Timestamp},
};

fn source(headers: &str, hold: bool) -> BmsChart {
    let channels = if hold {
        "#00051:0101\n#00056:0101\n"
    } else {
        "#00011:01\n#00016:01\n"
    };
    beatkernel_bms::parse(
        &format!("#BPM 120\n#WAV01 note.wav\n{headers}\n{channels}"),
        Default::default(),
    )
    .unwrap()
}
fn selection(precedence: BmsRankPrecedence) -> TimingPresetSelection {
    TimingPresetSelection {
        preset: BmsTimingPreset::BeatorajaSevenKeys8320241dV1,
        precedence,
    }
}

#[test]
fn preset_parser_and_gauge_selection_are_explicit_and_independent() {
    let id = BmsTimingPreset::BeatorajaSevenKeys8320241dV1.id();
    let selected = TimingPresetSelection::parse(id, "rank-first").unwrap();
    assert_eq!(selected, selection(BmsRankPrecedence::RankFirst));
    assert_eq!(
        TimingPresetSelection::parse(id, "defexrank-first")
            .unwrap()
            .precedence,
        BmsRankPrecedence::DefExRankFirst
    );
    for (id, precedence) in [
        ("", "rank-first"),
        ("latest", "rank-first"),
        (id, ""),
        (id, "guess"),
    ] {
        assert!(TimingPresetSelection::parse(id, precedence).is_err());
    }
    let source = source("#RANK 3", false);
    let policy =
        ResolvedPlayPolicy::with_timing(&source, GaugeSelection::BeatKernel, selected, 0).unwrap();
    assert_eq!(policy.selection(), GaugeSelection::BeatKernel);
    assert_eq!(policy.gauge(), &GaugeProfile::default());
    assert_eq!(policy.total(), None);
    assert_eq!(policy.judge().windows().len(), 4);
    assert!(policy.judgments().is_some());
}

#[test]
fn original_gauge_context_survives_practice_filtering() {
    let source = source("#RANK 3", true);
    let context = OriginalGaugeContext::from_source(&source);
    let filtered = crate::section_start::source_at(&source, Timestamp::from_nanos(1)).unwrap();
    assert_eq!(filtered.judged_stage_count(), 0);
    let full = ResolvedPlayPolicy::bms_with_timing(
        &source,
        BmsGaugeKind::Groove,
        selection(BmsRankPrecedence::RankFirst),
        0,
    )
    .unwrap();
    let section = ResolvedPlayPolicy::from_context_with_timing(
        &context,
        &filtered,
        GaugeSelection::Bms(BmsGaugeKind::Groove),
        selection(BmsRankPrecedence::RankFirst),
        0,
    )
    .unwrap();
    assert_eq!(full, section);
}

#[test]
fn preset_resolves_classified_windows_and_retains_explicit_precedence() {
    let source = source("#RANK 2\n#DEFEXRANK 100", false);
    let rank = ResolvedPlayPolicy::bms_with_timing(
        &source,
        BmsGaugeKind::Groove,
        selection(BmsRankPrecedence::RankFirst),
        -23,
    )
    .unwrap();
    let defex = ResolvedPlayPolicy::bms_with_timing(
        &source,
        BmsGaugeKind::Groove,
        selection(BmsRankPrecedence::DefExRankFirst),
        -23,
    )
    .unwrap();
    assert_eq!(rank.judge(), defex.judge());
    assert_eq!(rank.gauge(), defex.gauge());
    assert_ne!(
        rank, defex,
        "equal envelopes do not erase declared policy identity"
    );
    assert_eq!(rank.judge().input_offset().as_nanos(), -23);
    assert_eq!(
        rank.timing()
            .unwrap()
            .profiles()
            .judge_profile(BmsTimingStage::KeyHead)
            .unwrap()
            .input_offset(),
        Duration::ZERO
    );
    assert_eq!(
        rank.timing().unwrap().interaction_semantics(),
        "beatkernel-hold/v1"
    );
    assert_eq!(rank.completion_late().as_nanos(), 217_500_000);
    assert_eq!(rank.total(), Some(source.gauge_total()));
    assert!(rank
        .judgments()
        .unwrap()
        .validate_profile(rank.judge())
        .is_ok());
}

#[test]
fn timing_selection_never_guesses_missing_or_unsupported_declarations() {
    for (headers, expected) in [
        ("", "requires declared"),
        ("#DEFEXRANK 0", "ZeroDefExRank"),
        ("#DEFEXRANK 100.5", "FractionalDefExRank"),
        ("#DEFEXRANK 999999999999999999", "Overflow"),
    ] {
        let source = source(headers, false);
        let before = source.metadata.clone();
        let error = ResolvedPlayPolicy::bms_with_timing(
            &source,
            BmsGaugeKind::Groove,
            selection(BmsRankPrecedence::RankFirst),
            0,
        )
        .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
        assert_eq!(source.metadata, before);
    }
    let mut invalid = source("#RANK 2\n#DEFEXRANK 100", false);
    invalid
        .metadata
        .insert("DEFEXRANK".into(), "invalid".into());
    assert!(matches!(
        ResolvedPlayPolicy::bms_with_timing(
            &invalid,
            BmsGaugeKind::Groove,
            selection(BmsRankPrecedence::RankFirst),
            0
        ),
        Err(PolicyError::Rank(_))
    ));
    assert!(ResolvedPlayPolicy::builtin(1, 2, 0)
        .unwrap()
        .timing()
        .is_none());
}

#[test]
fn native_policy_preparation_executes_selected_heads_and_tails_with_one_offset() {
    let source = source("#RANK 3", true);
    let policy = ResolvedPlayPolicy::bms_with_timing(
        &source,
        BmsGaugeKind::Groove,
        selection(BmsRankPrecedence::RankFirst),
        5_000_000,
    )
    .unwrap();
    let config = crate::native_judge::NativeJudgeConfig {
        early: 1,
        late: 1,
        offset: 0,
        preroll: 0,
        output: ClockDomainId(1),
        end: None,
    };
    let mut judge = config
        .judge_with_policy(&source, source.compile().unwrap().chart, &policy)
        .unwrap();
    policy
        .validate_timing(&judge, beatkernel_bms::BmsInputMode::ButtonOnly)
        .unwrap();
    let button = |control, state| GameInputEvent {
        game_control: GameControlId(control),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(
                DeviceId(1),
                ClockPoint {
                    domain: ClockDomainId(1),
                    timestamp: Timestamp::ZERO,
                },
                0,
            ),
            control: PhysicalControlId::keyboard(control as u16),
            state,
        }),
    };
    for (control, expected) in [(0x11, 2), (0x16, 1)] {
        let events = judge
            .push_input(
                &button(control, ButtonState::Down),
                Timestamp::from_nanos(20_000_000),
            )
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].stage, JudgeStage::HoldHead);
        assert!(
            matches!(events[0].outcome, JudgeOutcome::Hit { grade, delta } if grade == JudgeGrade(expected) && delta.as_nanos() == 25_000_000)
        );
    }
    let tail = source.compile().unwrap().chart.objects()[0]
        .time
        .end
        .unwrap()
        .as_nanos();
    for (control, expected) in [(0x11, 2), (0x16, 1)] {
        let events = judge
            .push_input(
                &button(control, ButtonState::Up),
                Timestamp::from_nanos(tail + 120_000_000),
            )
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].stage, JudgeStage::HoldTail);
        assert!(
            matches!(events[0].outcome, JudgeOutcome::Hit { grade, delta } if grade == JudgeGrade(expected) && delta.as_nanos() == 125_000_000)
        );
    }
}

#[test]
fn same_envelope_wrong_stages_or_contacts_are_refused_without_recording() {
    use beatkernel::judge::JudgeEngine;
    use beatkernel_bms::BmsInputMode;
    let source = source("#RANK 3", true);
    let policy = ResolvedPlayPolicy::bms_with_timing(
        &source,
        BmsGaugeKind::Groove,
        selection(BmsRankPrecedence::RankFirst),
        0,
    )
    .unwrap();
    let make = |rules| {
        JudgeEngine::new(
            source.compile().unwrap().chart,
            rules,
            policy.judge().clone(),
        )
        .unwrap()
    };
    let legacy = make(source.rules());
    assert_eq!(legacy.profile(), policy.judge());
    assert!(policy
        .validate_timing(&legacy, BmsInputMode::ButtonOnly)
        .is_err());
    let different = BmsTimingPreset::BeatorajaSevenKeys8320241dV1
        .resolve(BmsJudgeDifficulty::Rank(beatkernel_bms::BmsRank::Normal))
        .unwrap();
    let wrong = make(
        source
            .rules_with_timing_profiles(BmsInputMode::ButtonOnly, &different)
            .unwrap(),
    );
    assert_eq!(wrong.profile(), policy.judge());
    assert!(policy
        .validate_timing(&wrong, BmsInputMode::ButtonOnly)
        .is_err());
    let contacts = make(
        source
            .rules_with_timing_profiles(
                BmsInputMode::ButtonOrContact,
                policy.timing().unwrap().profiles(),
            )
            .unwrap(),
    );
    assert!(policy
        .validate_timing(&contacts, BmsInputMode::ButtonOnly)
        .is_err());
    policy
        .validate_timing(&contacts, BmsInputMode::ButtonOrContact)
        .unwrap();
    let classified = policy
        .judge()
        .windows()
        .iter()
        .zip([
            BmsJudgment::PGreat,
            BmsJudgment::Great,
            BmsJudgment::Good,
            BmsJudgment::Bad,
        ])
        .map(|(window, judgment)| ClassifiedWindow {
            judgment,
            window: *window,
        })
        .collect::<Vec<_>>();
    let legacy_policy =
        ResolvedPlayPolicy::bms(&source, BmsGaugeKind::Groove, &classified, 0).unwrap();
    assert_eq!(legacy_policy.judge(), policy.judge());
    assert_eq!(legacy_policy.gauge(), policy.gauge());
    assert!(
        legacy_policy
            .validate_timing(&contacts, BmsInputMode::ButtonOrContact)
            .is_err(),
        "actual staged execution cannot lose its selected policy identity"
    );
    legacy_policy
        .validate_timing(&legacy, BmsInputMode::ButtonOnly)
        .unwrap();
}
