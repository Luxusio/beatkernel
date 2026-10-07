use super::*;
use beatkernel::{
    chart::ObjectId,
    judge::{JudgeStage, MissReason},
    time::{Duration, Timestamp},
};
use beatkernel_bms::{BmsGaugeKind, BmsGaugeRules, BmsJudgment, BmsTotal};
fn hit(grade: u32) -> JudgeEvent {
    JudgeEvent {
        object: ObjectId(1),
        stage: JudgeStage::Instant,
        outcome: JudgeOutcome::Hit {
            grade: JudgeGrade(grade),
            delta: Duration::ZERO,
        },
        at: Timestamp::ZERO,
        input: None,
    }
}
fn miss() -> JudgeEvent {
    JudgeEvent {
        outcome: JudgeOutcome::Miss {
            reason: MissReason::HeadTimeout,
        },
        ..hit(0)
    }
}

#[test]
fn resolved_adapter_judgments_match_runtime_dynamics_for_every_variant() {
    for kind in BmsGaugeKind::ALL {
        let rules =
            BmsGaugeRules::lr2(kind, BmsTotal::from_units(300_000_000).unwrap(), 1000).unwrap();
        let profile = GaugeProfile::from_bms_rules(
            rules,
            BmsJudgment::PGreat,
            &[
                (JudgeGrade(u32::MAX), BmsJudgment::Bad),
                (JudgeGrade(0), BmsJudgment::Good),
                (JudgeGrade(77), BmsJudgment::Great),
                (JudgeGrade(88), BmsJudgment::EmptyPoor),
            ],
        )
        .unwrap();
        let mut runtime = BmsGauge::new(profile);
        let mut reference = rules.start();
        for judgment in BmsJudgment::ALL.into_iter().cycle().take(1000) {
            let event = match judgment {
                BmsJudgment::PGreat => hit(7),
                BmsJudgment::Good => hit(0),
                BmsJudgment::Great => hit(77),
                BmsJudgment::EmptyPoor => hit(88),
                BmsJudgment::Bad => hit(u32::MAX),
                _ => miss(),
            };
            runtime.observe(&[event], &[]).unwrap();
            reference.apply(judgment);
            assert_eq!(runtime.snapshot().level_units, reference.level() as u64);
            assert_eq!(runtime.can_clear(), reference.qualified());
        }
    }
}

#[test]
fn reduction_is_strict_and_wide_deltas_never_overflow_or_recover_failure() {
    let dynamics = GaugeDynamics {
        minimum_alive: 0,
        failure_below: 2_000_000,
        damage_reduction_below: 32_000_000,
    };
    for (initial, expected) in [(32_000_000, 22_000_000), (31_999_999, 25_999_999)] {
        let profile = GaugeProfile::new(initial, 0, 1, -10_000_000, true, vec![])
            .unwrap()
            .with_dynamics(dynamics)
            .unwrap();
        let mut gauge = BmsGauge::new(profile);
        gauge.observe(&[miss()], &[]).unwrap();
        assert_eq!(gauge.snapshot().level_units, expected);
    }
    let profile = GaugeProfile::new(3_000_000, 0, i64::MAX, i64::MIN, true, vec![])
        .unwrap()
        .with_dynamics(dynamics)
        .unwrap();
    let mut gauge = BmsGauge::new(profile);
    gauge.observe(&[miss(), hit(1)], &[]).unwrap();
    assert_eq!(
        *gauge.snapshot(),
        GaugeSnapshot {
            level_units: 0,
            failure: Some(GaugeFailure::Depleted)
        }
    );
    let floor = GaugeDynamics {
        minimum_alive: 2_000_000,
        failure_below: 0,
        damage_reduction_below: 0,
    };
    let mut gauge = BmsGauge::new(
        GaugeProfile::new(20_000_000, 80_000_000, 1, i64::MIN, false, vec![])
            .unwrap()
            .with_dynamics(floor)
            .unwrap(),
    );
    gauge.observe(&[miss()], &[]).unwrap();
    assert_eq!(
        *gauge.snapshot(),
        GaugeSnapshot {
            level_units: 2_000_000,
            failure: None
        }
    );
}

#[test]
fn invalid_dynamics_and_duplicate_opaque_grade_maps_refuse_during_setup() {
    let profile = GaugeProfile::default();
    assert!(
        profile
            .clone()
            .with_dynamics(GaugeDynamics {
                minimum_alive: MAX_GAUGE_UNITS + 1,
                ..GaugeDynamics::default()
            })
            .is_err()
    );
    assert!(
        profile
            .with_dynamics(GaugeDynamics {
                failure_below: 1,
                ..GaugeDynamics::default()
            })
            .is_err()
    );
    let rules = BmsGaugeRules::lr2(
        BmsGaugeKind::Groove,
        BmsTotal::from_units(300_000_000).unwrap(),
        1000,
    )
    .unwrap();
    assert!(
        GaugeProfile::from_bms_rules(
            rules,
            BmsJudgment::PGreat,
            &[
                (JudgeGrade(1), BmsJudgment::Good),
                (JudgeGrade(1), BmsJudgment::Bad)
            ]
        )
        .is_err()
    );
}

#[test]
fn mine_damage_remains_raw_and_invalid_hazards_are_atomic_under_dynamic_profile() {
    use beatkernel::{
        input::GameControlId,
        judge::{HazardId, HazardOutcome},
    };
    let profile = GaugeProfile::new(20_000_000, 0, 1_000_000, -10_000_000, true, vec![])
        .unwrap()
        .with_dynamics(GaugeDynamics {
            minimum_alive: 0,
            failure_below: 2_000_000,
            damage_reduction_below: 32_000_000,
        })
        .unwrap();
    let mut gauge = BmsGauge::new(profile);
    let hazard = HazardEvent {
        id: HazardId(1),
        at: Timestamp::ZERO,
        control: GameControlId(1),
        value: 20,
        outcome: HazardOutcome::Triggered,
        input: None,
    };
    let before = *gauge.snapshot();
    assert!(
        gauge
            .observe(&[hit(1)], &[HazardEvent { value: 0, ..hazard }])
            .is_err()
    );
    assert_eq!(*gauge.snapshot(), before);
    gauge.observe(&[], &[hazard]).unwrap();
    assert_eq!(gauge.snapshot().level_units, 10_000_000);
    gauge
        .observe(
            &[],
            &[HazardEvent {
                value: 1295,
                ..hazard
            }],
        )
        .unwrap();
    assert_eq!(
        *gauge.snapshot(),
        GaugeSnapshot {
            level_units: 0,
            failure: Some(GaugeFailure::InstantDeath)
        }
    );
}
