//! Table-driven LR2 gauge fixtures: #TOTAL resolution, exact per-judgment
//! deltas, damage multipliers, guts, floors and latched survival failure.
use beatkernel_bms::{
    BmsGaugeError, BmsGaugeKind, BmsGaugeRules, BmsJudgment, BmsTotal, GAUGE_UNITS_PER_PERCENT,
    MAX_GAUGE_LEVEL, ParseOptions, ResolvedTotal, TOTAL_UNITS, TotalSource, parse,
};
use BmsGaugeKind::*;
use BmsJudgment::*;
use beatkernel_bms::{DuplicatePolicy, parse_seeded};

const P: i64 = GAUGE_UNITS_PER_PERCENT;

#[test]
fn total_resolution_uses_selected_headers_and_existing_duplicate_policy() {
    let source = "#BPM 120\n#WAV01 a.wav\n#00011:01\n#RANDOM 2\n#IF 1\n#TOTAL invalid\n#ELSE\n#TOTAL 250\n#ENDIF\n#ENDRANDOM";
    let selected = parse_seeded(source, ParseOptions::default(), 0).unwrap();
    assert_eq!(selected.gauge_total().source, TotalSource::Declared);
    assert_eq!(selected.gauge_total().total, total(250));
    let invalid = parse_seeded(source, ParseOptions::default(), 3).unwrap();
    assert_eq!(invalid.gauge_total().source, TotalSource::Invalid);
    assert_eq!(invalid.gauge_total().total, BmsTotal::lr2_default(1));
    assert_eq!(selected.compile().unwrap(), invalid.compile().unwrap());

    let duplicate = "#BPM 120\n#WAV01 a.wav\n#00011:01\n#TOTAL 200\n#TOTAL 300";
    assert_eq!(
        parse(duplicate, ParseOptions::default()).unwrap_err().line,
        5
    );
    let last = parse(
        duplicate,
        ParseOptions {
            duplicates: DuplicatePolicy::LastWins,
            ..ParseOptions::default()
        },
    )
    .unwrap();
    assert_eq!(last.gauge_total().total, total(300));
}

#[test]
fn maximum_public_inputs_and_fractional_recovery_remain_bounded() {
    let max_total = BmsTotal::from_units(u64::MAX).unwrap();
    for kind in BmsGaugeKind::ALL {
        let mut state = BmsGaugeRules::lr2(kind, max_total, u64::MAX)
            .unwrap()
            .start();
        for judgment in BmsJudgment::ALL {
            state.apply(judgment);
            assert!((0..=MAX_GAUGE_LEVEL).contains(&state.level()));
        }
    }
    // A sub-percent TOTAL does not invent recovery lost to per-stage truncation.
    let tiny = BmsTotal::from_units(1).unwrap();
    let rules = BmsGaugeRules::lr2(Groove, tiny, 3).unwrap();
    assert_eq!(rules.delta(PGreat), 0);
    let mut state = rules.start();
    for _ in 0..3 {
        state.apply(PGreat);
    }
    assert_eq!(state.level(), 20 * P);
}

fn total(points: u64) -> BmsTotal {
    BmsTotal::from_units(points * TOTAL_UNITS).unwrap()
}
fn rules(kind: BmsGaugeKind, total_points: u64, notes: u64) -> BmsGaugeRules {
    BmsGaugeRules::lr2(kind, total(total_points), notes).unwrap()
}
fn deltas(rules: &BmsGaugeRules) -> [i64; 6] {
    BmsJudgment::ALL.map(|judgment| rules.delta(judgment))
}

#[test]
fn total_headers_parse_exact_positive_plain_decimals_and_reject_everything_else() {
    for (text, units) in [
        ("300", Some(300_000_000)),
        ("+250.5", Some(250_500_000)),
        (" 160 ", Some(160_000_000)),
        ("1.23456789", Some(1_234_567)),
        ("0.000001", Some(1)),
        ("18446744073709", Some(18_446_744_073_709_000_000)),
        ("18446744073710", None),
        ("0", None),
        ("0.0000009", None),
        ("-100", None),
        ("1e3", None),
        ("12.3.4", None),
        ("", None),
        ("abc", None),
        ("1234567890123456789", None),
    ] {
        assert_eq!(
            BmsTotal::parse(text).map(BmsTotal::units),
            units,
            "{text:?}"
        );
    }
    assert_eq!(BmsTotal::from_units(0), None);
}

#[test]
fn absent_or_invalid_total_uses_the_lr2_note_count_default_with_provenance() {
    for (notes, units) in [
        (0, 160_000_000),
        (100, 176_000_000),
        (400, 224_000_000),
        (401, 224_320_000),
        (500, 256_000_000),
        (600, 288_000_000),
        (1000, 352_000_000),
        (u64::MAX, u64::MAX),
    ] {
        assert_eq!(BmsTotal::lr2_default(notes).units(), units, "{notes}");
    }
    for (declared, source, units) in [
        (None, TotalSource::Absent, 352_000_000),
        (Some("bogus"), TotalSource::Invalid, 352_000_000),
        (Some("0"), TotalSource::Invalid, 352_000_000),
        (Some("-5"), TotalSource::Invalid, 352_000_000),
        (Some("300"), TotalSource::Declared, 300_000_000),
    ] {
        let resolved = ResolvedTotal::resolve(declared, 1000);
        assert_eq!((resolved.source, resolved.total.units()), (source, units));
    }
}

#[test]
fn charts_count_judged_stages_and_resolve_preserved_total_metadata() {
    let body = "#BPM 60\n#LNTYPE 1\n#WAV01 a.wav\n#00012:0101\n#00051:0001\n#00151:0002\n\
                #000D1:001E00\n#00031:01\n";
    let declared = parse(&format!("#TOTAL 250\n{body}"), ParseOptions::default()).unwrap();
    // Two instants plus one hold head/tail; mines and invisible keys are unjudged.
    assert_eq!(declared.judged_stage_count(), 4);
    assert_eq!(
        declared.gauge_total(),
        ResolvedTotal {
            total: total(250),
            source: TotalSource::Declared
        }
    );
    let groove = declared.lr2_gauge_rules(Groove).unwrap();
    assert_eq!(groove.delta(PGreat), 62_500_000);
    let absent = parse(body, ParseOptions::default()).unwrap();
    assert_eq!(absent.gauge_total().source, TotalSource::Absent);
    assert_eq!(absent.gauge_total().total.units(), 160_640_000);
    let invalid = parse(&format!("#TOTAL zero\n{body}"), ParseOptions::default()).unwrap();
    assert_eq!(invalid.gauge_total().source, TotalSource::Invalid);
    assert_eq!(invalid.gauge_total().total.units(), 160_640_000);
    let empty = parse("#BPM 60\n#TOTAL 300", ParseOptions::default()).unwrap();
    assert_eq!(empty.judged_stage_count(), 0);
    assert_eq!(
        empty.lr2_gauge_rules(Hard),
        Err(BmsGaugeError::NoJudgedNotes)
    );
}

#[test]
fn every_variant_resolves_exact_per_judgment_deltas_and_thresholds() {
    // TOTAL 300 over 1000 stages: groove recovery is 0.3% per PGREAT and the
    // LR2 survival damage multiplier is exactly one.
    for (kind, expected, [initial, minimum, death, clear, guts]) in [
        (
            AssistEasy,
            [
                360_000, 360_000, 180_000, -3_200_000, -4_800_000, -1_600_000,
            ],
            [20, 2, 0, 60, 0],
        ),
        (
            Easy,
            [
                360_000, 360_000, 180_000, -3_200_000, -4_800_000, -1_600_000,
            ],
            [20, 2, 0, 80, 0],
        ),
        (
            Groove,
            [
                300_000, 300_000, 150_000, -4_000_000, -6_000_000, -2_000_000,
            ],
            [20, 2, 0, 80, 0],
        ),
        (
            Hard,
            [
                100_000,
                100_000,
                50_000,
                -6_000_000,
                -10_000_000,
                -2_000_000,
            ],
            [100, 0, 2, 0, 32],
        ),
        (
            ExHard,
            [
                100_000,
                100_000,
                50_000,
                -12_000_000,
                -20_000_000,
                -2_000_000,
            ],
            [100, 0, 2, 0, 0],
        ),
        (
            Hazard,
            [150_000, 60_000, 0, -100_000_000, -100_000_000, -10_000_000],
            [100, 0, 2, 0, 0],
        ),
    ] {
        let rules = rules(kind, 300, 1000);
        assert_eq!(rules.kind(), kind);
        assert_eq!(deltas(&rules), expected, "{kind:?}");
        assert_eq!(
            [
                rules.initial_level(),
                rules.minimum_level(),
                rules.death_level(),
                rules.clear_level(),
                rules.guts_below()
            ],
            [initial, minimum, death, clear, guts].map(|percent| percent * P),
            "{kind:?}"
        );
        assert_eq!(kind.is_survival(), initial == 100);
        assert_eq!(rules.start().level(), initial * P);
    }
    assert_eq!(BmsGaugeKind::ALL.len(), 6);
}

#[test]
fn groove_recovery_scales_by_total_per_stage_truncating_and_saturating() {
    let thirds = rules(Groove, 200, 3);
    assert_eq!(&deltas(&thirds)[..3], [66_666_666, 66_666_666, 33_333_333]);
    let huge = BmsGaugeRules::lr2(Easy, BmsTotal::from_units(u64::MAX).unwrap(), 1).unwrap();
    assert_eq!(&deltas(&huge)[..3], [MAX_GAUGE_LEVEL; 3]);
    // Damage is never TOTAL-scaled on groove-family or Hazard gauges.
    assert_eq!(rules(Groove, 160, 400).delta(Bad), -4 * P);
    assert_eq!(rules(Hazard, 160, 400).delta(Bad), -100 * P);
    assert_eq!(
        BmsGaugeRules::lr2(Groove, total(300), 0),
        Err(BmsGaugeError::NoJudgedNotes)
    );
}

#[test]
fn survival_damage_uses_the_larger_total_or_note_count_multiplier() {
    for (total_units, notes, poor, exhard_poor) in [
        (300_000_000, 5000, -10_000_000, -20_000_000),
        (240_000_000, 1000, -10_000_000, -20_000_000),
        (239_999_000, 1000, -11_111_111, -22_222_222),
        (160_000_000, 1000, -20_000_000, -40_000_000),
        (95_000_000, 1000, -100_000_000, -200_000_000),
        (1, 1000, -100_000_000, -200_000_000),
        (300_000_000, 20, -100_000_000, -200_000_000),
        (300_000_000, 21, -98_000_000, -196_000_000),
        (300_000_000, 29, -82_000_000, -164_000_000),
        (300_000_000, 30, -70_000_000, -140_000_000),
        (300_000_000, 59, -50_666_666, -101_333_333),
        (300_000_000, 60, -50_000_000, -100_000_000),
        (300_000_000, 124, -40_153_846, -80_307_692),
        (300_000_000, 125, -40_000_000, -80_000_000),
        (300_000_000, 249, -30_080_000, -60_160_000),
        (300_000_000, 250, -30_000_000, -60_000_000),
        (300_000_000, 499, -20_040_000, -40_080_000),
        (300_000_000, 500, -20_000_000, -40_000_000),
        (300_000_000, 999, -10_020_000, -20_040_000),
        (160_000_000, 750, -20_000_000, -40_000_000),
        (160_000_000, 400, -24_000_000, -48_000_000),
    ] {
        let total = BmsTotal::from_units(total_units).unwrap();
        let hard = BmsGaugeRules::lr2(Hard, total, notes).unwrap();
        assert_eq!(hard.delta(Poor), poor, "{total_units} {notes}");
        assert_eq!(hard.delta(PGreat), 100_000);
        let exhard = BmsGaugeRules::lr2(ExHard, total, notes).unwrap();
        assert_eq!(exhard.delta(Poor), exhard_poor, "{total_units} {notes}");
    }
    assert_eq!(rules(Hard, 160, 400).delta(Bad), -14_400_000);
}

#[test]
fn groove_family_floors_at_two_percent_and_clears_only_at_its_border() {
    let mut gauge = rules(Groove, 300, 1000).start();
    for _ in 0..4 {
        gauge.apply(Poor);
    }
    assert_eq!(gauge.level(), 2 * P);
    assert!(!gauge.failed() && !gauge.qualified());
    for _ in 0..259 {
        gauge.apply(PGreat);
    }
    assert_eq!(gauge.level(), 79_700_000);
    assert!(!gauge.qualified());
    gauge.apply(Great);
    assert_eq!(gauge.level(), 80 * P);
    assert!(gauge.qualified());
    for _ in 0..200 {
        gauge.apply(Good);
    }
    assert_eq!(gauge.level(), MAX_GAUGE_LEVEL);
    let mut assist = rules(AssistEasy, 300, 1000).start();
    for _ in 0..134 {
        assist.apply(PGreat);
    }
    assert_eq!(assist.level(), 68_240_000);
    assert!(assist.qualified());
    assert!(!rules(Easy, 300, 1000).start().qualified());
}

#[test]
fn hard_guts_apply_strictly_below_32_percent_and_death_latches_below_two() {
    let mut gauge = rules(Hard, 300, 1000).start();
    let mut trace = Vec::new();
    for judgment in [
        Poor, Poor, Poor, Poor, Poor, Poor, Bad, EmptyPoor, Poor, Poor,
    ] {
        gauge.apply(judgment);
        trace.push(gauge.level() / 100_000);
    }
    // 34% -> 32% stays unreduced; at 32% a POOR still costs 10%; at 22% it costs 6%.
    assert_eq!(trace, [900, 800, 700, 600, 500, 400, 340, 320, 220, 160]);
    for _ in 0..2 {
        gauge.apply(Poor);
    }
    assert_eq!(gauge.level(), 4 * P);
    gauge.apply(EmptyPoor);
    assert_eq!(gauge.level(), 2_800_000);
    gauge.apply(PGreat);
    assert_eq!(gauge.level(), 2_900_000);
    gauge.apply(EmptyPoor);
    assert_eq!(gauge.level(), 0);
    assert!(gauge.failed() && !gauge.qualified());
    let frozen = gauge;
    for judgment in BmsJudgment::ALL {
        gauge.apply(judgment);
    }
    assert_eq!(gauge, frozen);
}

#[test]
fn exhard_and_hazard_fail_on_exact_damage_without_guts() {
    let mut exhard = rules(ExHard, 300, 1000).start();
    for _ in 0..4 {
        exhard.apply(Poor);
    }
    assert_eq!(exhard.level(), 20 * P);
    assert!(exhard.qualified());
    exhard.apply(Poor);
    assert!(exhard.failed());
    let mut hazard = rules(Hazard, 300, 1000).start();
    hazard.apply(PGreat);
    assert_eq!(hazard.level(), MAX_GAUGE_LEVEL);
    for _ in 0..9 {
        hazard.apply(EmptyPoor);
    }
    assert_eq!(hazard.level(), 10 * P);
    assert!(hazard.qualified());
    hazard.apply(EmptyPoor);
    assert!(hazard.failed());
    for judgment in [Bad, Poor] {
        let mut single = rules(Hazard, 300, 1000).start();
        single.apply(judgment);
        assert!(single.failed(), "{judgment:?}");
    }
    let mut good = rules(Hazard, 300, 1000).start();
    good.apply(Good);
    assert_eq!(good.level(), MAX_GAUGE_LEVEL);
}
