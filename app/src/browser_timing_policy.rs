//! Pure browser setup bridge shared by the WASM entry point and portable tests.

use crate::{
    PreparedBms,
    play_policy::{GaugeSelection, PolicyError, ResolvedPlayPolicy, TimingPresetSelection},
};
use beatkernel::{audio::PcmLimits, time::Timestamp};

pub(crate) fn parse_selection(
    preset_id: &str,
    precedence: &str,
    gauge: &str,
) -> Result<(TimingPresetSelection, GaugeSelection), PolicyError> {
    Ok((
        TimingPresetSelection::parse(preset_id, precedence)?,
        gauge.parse()?,
    ))
}

pub(crate) fn prepare_selected_at(
    original: PreparedBms,
    start: Timestamp,
    limits: PcmLimits,
    selection: (TimingPresetSelection, GaugeSelection),
    offset_ns: i64,
) -> Result<(PreparedBms, ResolvedPlayPolicy), Box<dyn std::error::Error>> {
    let (timing, gauge) = selection;
    let policy = ResolvedPlayPolicy::with_timing(&original.source, gauge, timing, offset_ns)?;
    let (prepared, _) = crate::section_start::prepare_at(original, start, limits)?;
    Ok((prepared, policy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatkernel::audio::{AudioFormat, SampleBank};
    use beatkernel_bms::{BmsInputMode, BmsJudgeDifficulty, BmsRank, BmsTimingPreset};

    const PRESET: BmsTimingPreset = BmsTimingPreset::BeatorajaSevenKeys8320241dV1;

    fn prepared(rank: &str) -> (PreparedBms, PcmLimits) {
        let source = beatkernel_bms::parse(
            &format!("#BPM 60\n#WAV01 note.wav\n{rank}\n#TOTAL 240\n#00011:0100\n#00116:01"),
            Default::default(),
        )
        .unwrap();
        let compiled = source.compile().unwrap();
        let limits = PcmLimits::new(64, 64, 1).unwrap();
        let bank = SampleBank::new(AudioFormat::new(1000, 1).unwrap(), limits).unwrap();
        (
            PreparedBms {
                source,
                compiled,
                bank,
                sounds: vec![],
                bgm_commands: vec![],
            },
            limits,
        )
    }

    #[test]
    fn explicit_browser_selection_rejects_missing_and_unknown_fields() {
        for (preset, precedence, gauge) in [
            ("", "rank-first", "beatkernel"),
            ("beatoraja-sevenkeys/v2", "rank-first", "beatkernel"),
            (PRESET.id(), "", "beatkernel"),
            (PRESET.id(), "last-header", "beatkernel"),
            (PRESET.id(), "rank-first", ""),
            (PRESET.id(), "rank-first", "unknown"),
        ] {
            assert!(parse_selection(preset, precedence, gauge).is_err());
        }
        assert!(parse_selection(PRESET.id(), "rank-first", "beatkernel").is_ok());
        assert!(parse_selection(PRESET.id(), "defexrank-first", "groove").is_ok());
    }

    #[test]
    fn browser_section_retains_original_gauge_policy_and_selected_offset() {
        let (original, limits) = prepared("#RANK 2");
        let selection = parse_selection(PRESET.id(), "rank-first", "groove").unwrap();
        let expected =
            ResolvedPlayPolicy::with_timing(&original.source, selection.1, selection.0, 37)
                .unwrap();
        let start = Timestamp::from_nanos(4_000_000_000);
        let (section, actual) =
            prepare_selected_at(original, start, limits, selection, 37).unwrap();
        assert_eq!(section.source.judged_stage_count(), 1);
        assert_eq!(actual, expected);
        assert_eq!(actual.judge().input_offset().as_nanos(), 37);
        let sliced_policy =
            ResolvedPlayPolicy::with_timing(&section.source, selection.1, selection.0, 37).unwrap();
        assert_ne!(actual.gauge(), sliced_policy.gauge());
        let judge = crate::mine_plan::prepare_judge_with_timing(
            &section.source,
            section.compiled.chart.clone(),
            actual.judge().clone(),
            BmsInputMode::ButtonOnly,
            beatkernel_bms::ParseOptions::default().max_objects,
            Some(actual.timing().unwrap().profiles()),
        )
        .unwrap();
        actual
            .timing()
            .unwrap()
            .validate_judge(&judge, BmsInputMode::ButtonOnly)
            .unwrap();
    }

    #[test]
    fn browser_policy_requires_declared_difficulty_and_keeps_explicit_baseline_gauge() {
        let selection = parse_selection(PRESET.id(), "rank-first", "beatkernel").unwrap();
        let (original, limits) = prepared("");
        assert!(prepare_selected_at(original, Timestamp::ZERO, limits, selection, 0).is_err());
        let (original, limits) = prepared("#RANK 3");
        let (_, policy) =
            prepare_selected_at(original, Timestamp::ZERO, limits, selection, 0).unwrap();
        assert_eq!(policy.selection(), GaugeSelection::BeatKernel);
        assert_eq!(policy.gauge(), &crate::gauge::GaugeProfile::default());
        assert_eq!(policy.timing().unwrap().selection().preset, PRESET);
    }

    #[test]
    fn browser_precedence_applies_to_actual_profiles_and_unsupported_percent_refuses() {
        for (precedence, rank_selected) in [("rank-first", true), ("defexrank-first", false)] {
            let selection = parse_selection(PRESET.id(), precedence, "beatkernel").unwrap();
            let (original, limits) = prepared("#RANK 1\n#DEFEXRANK 100");
            let (_, policy) =
                prepare_selected_at(original, Timestamp::ZERO, limits, selection, 0).unwrap();
            let difficulty = policy.timing().unwrap().profiles().difficulty();
            if rank_selected {
                assert_eq!(difficulty, BmsJudgeDifficulty::Rank(BmsRank::Hard));
            } else {
                assert!(matches!(difficulty, BmsJudgeDifficulty::DefExRank(value)
                    if value.numerator() == 100 && value.denominator() == 1));
            }
        }
        let selection = parse_selection(PRESET.id(), "defexrank-first", "beatkernel").unwrap();
        for percent in ["0", "120.125"] {
            let (original, limits) = prepared(&format!("#RANK 1\n#DEFEXRANK {percent}"));
            assert!(prepare_selected_at(original, Timestamp::ZERO, limits, selection, 0).is_err());
        }
    }
}
