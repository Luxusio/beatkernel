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
