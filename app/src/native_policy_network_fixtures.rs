//! Selected network metadata is checked even without a replay recorder.
use crate::{
    gauge::BmsGauge,
    native_gameplay::NativeGameplayConfig,
    play_policy::{ClassifiedWindow, ResolvedPlayPolicy},
};
use beatkernel::{
    judge::{JudgeEngine, JudgeGrade, JudgeWindow},
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::{BmsGaugeKind, BmsJudgment};

fn source() -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(
        "#BPM 60\n#TOTAL 320\n#WAV01 note.wav\n#00011:0101",
        Default::default(),
    )
    .unwrap()
}
fn policy(
    source: &beatkernel_bms::BmsChart,
    kind: BmsGaugeKind,
    class: BmsJudgment,
    offset: i64,
) -> ResolvedPlayPolicy {
    ResolvedPlayPolicy::bms(
        source,
        kind,
        &[ClassifiedWindow {
            judgment: class,
            window: JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::from_nanos(7),
                late: Duration::from_nanos(9),
            },
        }],
        offset,
    )
    .unwrap()
}
fn judge(source: &beatkernel_bms::BmsChart, policy: &ResolvedPlayPolicy) -> JudgeEngine {
    JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        policy.judge().clone(),
    )
    .unwrap()
}
fn config() -> NativeGameplayConfig {
    let point = |domain| ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::ZERO,
    };
    NativeGameplayConfig {
        origin: point(17),
        stream_origin: point(2),
        playback_origin: point(2),
        song_origin: Timestamp::ZERO,
        sample_rate: 1000,
        end_song: Some(Timestamp::from_nanos(2_000_000_000)),
        advance_lag: Duration::ZERO,
        seconds: None,
        pause_supported: false,
        logical_schedule: true,
    }
}
fn header(
    judge: &JudgeEngine,
    policy: &ResolvedPlayPolicy,
    start: Timestamp,
    end: Option<Timestamp>,
) -> beatkernel::replay::ReplayHeader {
    crate::replay_capture::setup_play_policy_header(
        judge,
        ClockDomainId(17),
        crate::competition_live::replay_limits().unwrap(),
        start,
        71,
        end,
        beatkernel_bms::BmsInputMode::ButtonOnly,
        None,
        policy,
    )
    .unwrap()
}

#[test]
fn selected_network_header_validates_without_capture_and_refuses_meaning_changes_before_judge_effects(
) {
    let source = source();
    let selected = policy(&source, BmsGaugeKind::Hard, BmsJudgment::Great, -3);
    let judge = judge(&source, &selected);
    let gauge = BmsGauge::new(selected.gauge().try_copy().unwrap());
    let cfg = config();
    let good = header(&judge, &selected, Timestamp::ZERO, cfg.end_song);
    crate::native_policy_admission::validate_selected(
        &judge,
        &gauge,
        &selected,
        None,
        Some(&good),
        &cfg,
    )
    .unwrap();
    let before = (judge.stable_hash().unwrap(), gauge.snapshot());
    let other_class = policy(&source, BmsGaugeKind::Hard, BmsJudgment::PGreat, -3);
    // Hard assigns these two hit classes the same gauge effects. Class identity
    // still differs, so comparing the gauge alone cannot admit this header.
    assert_eq!(selected.gauge(), other_class.gauge());
    let other_gauge = policy(&source, BmsGaugeKind::Hazard, BmsJudgment::Great, -3);
    let other_profile = policy(&source, BmsGaugeKind::Hard, BmsJudgment::Great, -2);
    let other_judge = self::judge(&source, &other_profile);
    let mismatches = [
        header(&judge, &other_class, Timestamp::ZERO, cfg.end_song),
        header(&judge, &other_gauge, Timestamp::ZERO, cfg.end_song),
        header(&other_judge, &other_profile, Timestamp::ZERO, cfg.end_song),
        header(&judge, &selected, Timestamp::from_nanos(1), cfg.end_song),
        header(&judge, &selected, Timestamp::ZERO, None),
    ];
    for mismatch in mismatches {
        assert!(crate::native_policy_admission::validate_selected(
            &judge,
            &gauge,
            &selected,
            None,
            Some(&mismatch),
            &cfg
        )
        .is_err());
        assert!(judge.effective_song_time().is_none());
        assert_eq!((judge.stable_hash().unwrap(), gauge.snapshot()), before);
    }
}

#[test]
fn disabled_selected_capture_still_refuses_mismatched_or_processed_judges() {
    let source = source();
    let selected = policy(&source, BmsGaugeKind::Hard, BmsJudgment::Great, 0);
    let wrong = policy(&source, BmsGaugeKind::Hard, BmsJudgment::Great, 1);
    let mut judge = judge(&source, &wrong);
    assert!(crate::native_judge::prepare_section_capture_for_policy(
        &source,
        &judge,
        &selected,
        ClockDomainId(17),
        Timestamp::ZERO,
        71,
        None,
        None
    )
    .is_err());
    assert!(judge.effective_song_time().is_none());
    assert!(crate::native_judge::prepare_section_capture_for_policy(
        &source,
        &judge,
        &wrong,
        ClockDomainId(17),
        Timestamp::ZERO,
        71,
        None,
        None
    )
    .unwrap()
    .is_none());
    judge.advance_to(Timestamp::ZERO).unwrap();
    assert!(crate::native_judge::prepare_section_capture_for_policy(
        &source,
        &judge,
        &wrong,
        ClockDomainId(17),
        Timestamp::ZERO,
        71,
        None,
        None
    )
    .is_err());
}

fn timing_source() -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(
        "#BPM 60\n#RANK 2\n#DEFEXRANK 100\n#TOTAL 320\n#LNOBJ 02\n#WAV01 note.wav\n#00011:0102\n#00016:0102",
        Default::default(),
    ).unwrap()
}
fn timing_selection(
    precedence: beatkernel_bms::BmsRankPrecedence,
) -> crate::play_policy::TimingPresetSelection {
    crate::play_policy::TimingPresetSelection {
        preset: beatkernel_bms::BmsTimingPreset::BeatorajaSevenKeys8320241dV1,
        precedence,
    }
}
fn timing_config() -> crate::native_judge::NativeJudgeConfig {
    crate::native_judge::NativeJudgeConfig {
        early: 0,
        late: 0,
        offset: -3,
        preroll: 0,
        output: ClockDomainId(17),
        end: None,
    }
}

#[test]
fn native_no_capture_rejects_both_staged_policy_rule_mismatches_before_mutation() {
    let source = timing_source();
    let selected = ResolvedPlayPolicy::bms_with_timing(
        &source,
        BmsGaugeKind::Hard,
        timing_selection(beatkernel_bms::BmsRankPrecedence::RankFirst),
        -3,
    )
    .unwrap();
    let legacy = ResolvedPlayPolicy::bms(
        &source,
        BmsGaugeKind::Hard,
        &selected
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
            .collect::<Vec<_>>(),
        -3,
    )
    .unwrap();
    assert_eq!(legacy.judge(), selected.judge());
    assert_eq!(legacy.gauge(), selected.gauge());
    let cfg = timing_config();
    let staged = cfg
        .judge_with_policy(&source, source.compile().unwrap().chart, &selected)
        .unwrap();
    let original = judge(&source, &legacy);
    let gauge = BmsGauge::new(selected.gauge().try_copy().unwrap());
    let gameplay = NativeGameplayConfig {
        end_song: None,
        ..config()
    };
    crate::native_policy_admission::validate_selected(
        &staged, &gauge, &selected, None, None, &gameplay,
    )
    .unwrap();
    let before = (
        staged.stable_hash().unwrap(),
        original.stable_hash().unwrap(),
        gauge.snapshot(),
    );
    assert!(crate::native_policy_admission::validate_selected(
        &original, &gauge, &selected, None, None, &gameplay
    )
    .is_err());
    assert!(crate::native_policy_admission::validate_selected(
        &staged, &gauge, &legacy, None, None, &gameplay
    )
    .is_err());
    for (actual, policy) in [(&original, &selected), (&staged, &legacy)] {
        assert!(crate::native_judge::prepare_section_capture_for_policy(
            &source,
            actual,
            policy,
            ClockDomainId(17),
            Timestamp::ZERO,
            0,
            None,
            None
        )
        .is_err());
    }
    assert_eq!(
        (
            staged.stable_hash().unwrap(),
            original.stable_hash().unwrap(),
            gauge.snapshot()
        ),
        before
    );
    assert!(staged.effective_song_time().is_none());
    assert!(original.effective_song_time().is_none());
}

#[test]
fn native_recorded_timing_identity_preserves_selected_declaration_even_same_effective_table() {
    let source = timing_source();
    let rank = ResolvedPlayPolicy::bms_with_timing(
        &source,
        BmsGaugeKind::Hard,
        timing_selection(beatkernel_bms::BmsRankPrecedence::RankFirst),
        -3,
    )
    .unwrap();
    let defex = ResolvedPlayPolicy::bms_with_timing(
        &source,
        BmsGaugeKind::Hard,
        timing_selection(beatkernel_bms::BmsRankPrecedence::DefExRankFirst),
        -3,
    )
    .unwrap();
    assert_eq!(rank.judge(), defex.judge());
    assert_eq!(rank.gauge(), defex.gauge());
    let actual = timing_config()
        .judge_with_policy(&source, source.compile().unwrap().chart, &rank)
        .unwrap();
    let gauge = BmsGauge::new(rank.gauge().try_copy().unwrap());
    let gameplay = NativeGameplayConfig {
        end_song: None,
        ..config()
    };
    let limits = crate::competition_live::replay_limits().unwrap();
    let capture = crate::native_judge::prepare_section_capture_for_policy(
        &source,
        &actual,
        &rank,
        ClockDomainId(17),
        Timestamp::ZERO,
        0,
        None,
        Some(limits),
    )
    .unwrap()
    .unwrap();
    let expected = crate::native_judge::prepare_policy_header(
        &source,
        &actual,
        &rank,
        ClockDomainId(17),
        Timestamp::ZERO,
        0,
        None,
    )
    .unwrap();
    assert_eq!(capture.header(), &expected);
    let decoded = crate::replay_playback::decode_section_setup(&expected.options).unwrap();
    assert_eq!(decoded.timing.as_ref(), rank.timing());
    crate::native_policy_admission::validate_selected(
        &actual,
        &gauge,
        &rank,
        Some(&capture),
        Some(&expected),
        &gameplay,
    )
    .unwrap();
    let wrong = crate::native_judge::prepare_policy_header(
        &source,
        &actual,
        &defex,
        ClockDomainId(17),
        Timestamp::ZERO,
        0,
        None,
    )
    .unwrap();
    assert_ne!(wrong.options, expected.options);
    assert!(crate::native_policy_admission::validate_selected(
        &actual,
        &gauge,
        &rank,
        None,
        Some(&wrong),
        &gameplay
    )
    .is_err());
    let encoded = beatkernel::replay::codec::encode_replay(&capture.into_file(), limits).unwrap();
    let file = beatkernel::replay::codec::decode_replay(&encoded, limits).unwrap();
    crate::replay_playback::validate_section_setup(&source, &file, limits).unwrap();
    crate::native_policy_admission::validate_header(
        &actual,
        gauge.profile(),
        &file.header,
        &gameplay,
    )
    .unwrap();
}

#[test]
fn native_source_aware_timing_checks_original_declaration_even_without_capture() {
    let source = timing_source();
    let selected = ResolvedPlayPolicy::bms_with_timing(
        &source,
        BmsGaugeKind::Hard,
        timing_selection(beatkernel_bms::BmsRankPrecedence::RankFirst),
        -3,
    )
    .unwrap();
    let actual = timing_config()
        .judge_with_policy(&source, source.compile().unwrap().chart, &selected)
        .unwrap();
    let hash = actual.stable_hash().unwrap();
    for change in [None, Some("bad"), Some("3")] {
        let mut wrong = source.clone();
        match change {
            None => {
                wrong.metadata.remove("RANK");
            }
            Some(value) => {
                wrong.metadata.insert("RANK".into(), value.into());
            }
        }
        // Removing RANK leaves DEFEX100 with the same effective windows; source
        // identity must still reject the different selected declaration.
        assert!(timing_config()
            .judge_with_policy(&wrong, wrong.compile().unwrap().chart, &selected)
            .is_err());
        assert!(crate::native_judge::prepare_section_capture_for_policy(
            &wrong,
            &actual,
            &selected,
            ClockDomainId(17),
            Timestamp::ZERO,
            0,
            None,
            None
        )
        .is_err());
        assert!(crate::native_judge::prepare_policy_header(
            &wrong,
            &actual,
            &selected,
            ClockDomainId(17),
            Timestamp::ZERO,
            0,
            None
        )
        .is_err());
    }
    assert_eq!(actual.stable_hash().unwrap(), hash);
    assert!(actual.effective_song_time().is_none());
}

#[test]
fn native_header_admission_cannot_infer_timing_from_matching_hash_or_envelope() {
    let source = timing_source();
    let selected = ResolvedPlayPolicy::bms_with_timing(
        &source,
        BmsGaugeKind::Hard,
        timing_selection(beatkernel_bms::BmsRankPrecedence::RankFirst),
        -3,
    )
    .unwrap();
    let staged = timing_config()
        .judge_with_policy(&source, source.compile().unwrap().chart, &selected)
        .unwrap();
    let legacy = judge(&source, &selected);
    let gameplay = NativeGameplayConfig {
        end_song: None,
        ..config()
    };
    let limits = crate::competition_live::replay_limits().unwrap();
    for (actual, record_timing) in [(&staged, false), (&legacy, true)] {
        // Deliberately bypass the complete-policy builder to construct a header
        // whose hash matches the actual judge but whose staged identity does not.
        let header = crate::replay_capture::setup_gauge_header(
            actual,
            ClockDomainId(17),
            limits,
            Timestamp::ZERO,
            0,
            None,
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            selected.gauge(),
        )
        .unwrap();
        let header =
            crate::replay_judgment_policy::wrap_header(header, selected.judgments(), limits)
                .unwrap();
        let header = crate::replay_capture::wrap_timing_header(
            header,
            record_timing.then(|| selected.timing().unwrap()),
            beatkernel_bms::BmsInputMode::ButtonOnly,
            limits,
        )
        .unwrap();
        assert!(crate::native_policy_admission::validate_header(
            actual,
            selected.gauge(),
            &header,
            &gameplay
        )
        .is_err());
    }
    let good = crate::native_judge::prepare_policy_header(
        &source,
        &staged,
        &selected,
        ClockDomainId(17),
        Timestamp::ZERO,
        0,
        None,
    )
    .unwrap();
    let mut wrong = good.clone();
    wrong.rules_identity = b"beatkernel-bms/builtin-judge/v1".to_vec();
    assert!(crate::native_policy_admission::validate_header(
        &staged,
        selected.gauge(),
        &wrong,
        &gameplay
    )
    .is_err());
    crate::native_policy_admission::validate_header(&staged, selected.gauge(), &good, &gameplay)
        .unwrap();
}
