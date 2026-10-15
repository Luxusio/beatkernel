//! Host-executable pure policy checks for the opt-in retained preparation helper.
//! Browser job exports and actual Worklet execution are covered in their own lanes.
use super::*;
use beatkernel::audio::{AudioFormat, SampleBank};
use beatkernel_bms::{BmsJudgeDifficulty, BmsRank, BmsTimingPreset};

const PRESET: BmsTimingPreset = BmsTimingPreset::BeatorajaSevenKeys8320241dV1;

fn original(rank_headers: &str) -> PreparedBms {
    let source = beatkernel_bms::parse(
        &format!("#BPM 60\n#WAV01 key.wav\n{rank_headers}\n#TOTAL 240\n#00011:0100\n#00116:01\n"),
        Default::default(),
    )
    .unwrap();
    let compiled = source.compile().unwrap();
    let bank = SampleBank::new(
        AudioFormat::new(1000, 1).unwrap(),
        PcmLimits::new(64, 64, 1).unwrap(),
    )
    .unwrap();
    PreparedBms {
        source,
        compiled,
        bank,
        sounds: vec![],
        bgm_commands: vec![],
    }
}

#[test]
fn retained_policy_uses_original_stage_count_and_exact_selected_offset() {
    let original = original("#RANK 2");
    let source = original.source.clone();
    let selection = parse_selection(PRESET.id(), "rank-first", "groove").unwrap();
    let expected = ResolvedPlayPolicy::with_timing(&source, selection.1, selection.0, -37).unwrap();
    let (retained, policy) = prepare_selected_retained_at(
        original,
        Timestamp::from_nanos(4_000_000_000),
        selection,
        -37,
    )
    .unwrap();

    assert_eq!(retained.original_source, source);
    assert_eq!(retained.section.original_gauge.judged_stages(), 2);
    assert_eq!(retained.prepared.source.judged_stage_count(), 1);
    assert_eq!(
        retained.prepared.compiled.chart.objects()[0].time.start,
        Timestamp::from_nanos(4_000_000_000)
    );
    assert_eq!(policy, expected);
    assert_eq!(policy.judge().input_offset().as_nanos(), -37);
    let selected_only =
        ResolvedPlayPolicy::with_timing(&retained.prepared.source, selection.1, selection.0, -37)
            .unwrap();
    assert_ne!(policy.gauge(), selected_only.gauge());
}

#[test]
fn retained_policy_preserves_explicit_rank_precedence_and_baseline_gauge() {
    for (precedence, use_rank) in [("rank-first", true), ("defexrank-first", false)] {
        let selection = parse_selection(PRESET.id(), precedence, "beatkernel").unwrap();
        let (retained, policy) = prepare_selected_retained_at(
            original("#RANK 1\n#DEFEXRANK 100"),
            Timestamp::from_nanos(4_000_000_000),
            selection,
            71,
        )
        .unwrap();
        assert_eq!(policy.selection(), GaugeSelection::BeatKernel);
        assert_eq!(policy.gauge(), &crate::gauge::GaugeProfile::default());
        assert_eq!(policy.judge().input_offset().as_nanos(), 71);
        assert_eq!(policy.timing().unwrap().selection(), selection.0);
        let difficulty = policy.timing().unwrap().profiles().difficulty();
        if use_rank {
            assert_eq!(difficulty, BmsJudgeDifficulty::Rank(BmsRank::Hard));
        } else {
            assert!(matches!(difficulty, BmsJudgeDifficulty::DefExRank(value)
                if value.numerator() == 100 && value.denominator() == 1));
        }
        assert_eq!(retained.original_source.judged_stage_count(), 2);
    }
}

#[test]
fn retained_policy_rejects_invalid_declared_policy_and_negative_start() {
    for (preset, precedence, gauge) in [
        ("", "rank-first", "beatkernel"),
        ("beatoraja-sevenkeys/v2", "rank-first", "beatkernel"),
        (PRESET.id(), "", "beatkernel"),
        (PRESET.id(), "rank-first", "unknown"),
    ] {
        assert!(parse_selection(preset, precedence, gauge).is_err());
    }
    let selection = parse_selection(PRESET.id(), "defexrank-first", "beatkernel").unwrap();
    for headers in ["", "#RANK 1\n#DEFEXRANK 0", "#RANK 1\n#DEFEXRANK 120.125"] {
        assert!(
            prepare_selected_retained_at(original(headers), Timestamp::ZERO, selection, 0).is_err()
        );
    }
    let mut malformed = original("#RANK 2");
    malformed
        .source
        .metadata
        .insert("RANK".into(), "invalid".into());
    assert!(prepare_selected_retained_at(malformed, Timestamp::ZERO, selection, 0).is_err());
    assert!(prepare_selected_retained_at(
        original("#RANK 2"),
        Timestamp::from_nanos(-1),
        selection,
        0,
    )
    .is_err());
}
