//! Pure original-song-time activation of actual accepted miss backgrounds.
use crate::{bga::BgaState, note_progress::NoteProgress, player_chart::PlayerChart};
use beatkernel::time::Timestamp;
use beatkernel_bms::{ImageId, PoorBgaMode};

/// Complete drawing intent: ordinary selections plus an activated raw Poor overlay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BgaPresentation {
    /// Base/Layer/Poor timeline selections, including replacement when active.
    pub state: BgaState,
    /// Raw Poor image drawn after Layer; no Layer black key is applied.
    pub poor_overlay: Option<ImageId>,
}
impl From<BgaState> for BgaPresentation {
    fn from(state: BgaState) -> Self {
        Self {
            state,
            poor_overlay: None,
        }
    }
}

/// Explicit BeatKernel display policy; it is not a hardware clock or BMS duration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoorBackgroundPolicy {
    lifetime_ns: i64,
}
impl Default for PoorBackgroundPolicy {
    fn default() -> Self {
        Self {
            lifetime_ns: 500_000_000,
        }
    }
}
impl PoorBackgroundPolicy {
    /// Selects 0 (off) through ten seconds, rejecting negative/out-of-cap values.
    pub fn new(lifetime_ns: i64) -> Result<Self, String> {
        if !(0..=10_000_000_000).contains(&lifetime_ns) {
            return Err("Poor background lifetime must be 0..=10 seconds".into());
        }
        Ok(Self { lifetime_ns })
    }
    /// Compatibility selection-only projection. Overlay mode requires select()
    /// to carry the separately activated Poor drawing intent.
    pub fn project(
        &self,
        chart: &PlayerChart,
        now: Timestamp,
        progress: Option<&NoteProgress>,
    ) -> Result<BgaState, String> {
        Ok(self.select(chart, now, progress)?.state)
    }
    /// Projects complete Replace/Overlay/Off intent within miss age [0,lifetime).
    /// Identity is always checked, including Off. Future misses wait for their
    /// effective time; seeking requires a reconstructed accepted prefix.
    pub fn select(
        &self,
        chart: &PlayerChart,
        now: Timestamp,
        progress: Option<&NoteProgress>,
    ) -> Result<BgaPresentation, String> {
        if progress.is_some_and(|state| !state.matches_chart(chart)) {
            return Err("Poor background progress belongs to another prepared chart".into());
        }
        let mut presentation = BgaPresentation::from(chart.bga_state(now));
        if let (Some(miss), Some(poor)) = (
            progress.and_then(NoteProgress::last_miss),
            presentation.state.poor,
        ) {
            let age = i128::from(now.as_nanos()) - i128::from(miss.as_nanos());
            if age >= 0 && age < i128::from(self.lifetime_ns) {
                match chart.poor_bga_mode {
                    PoorBgaMode::Replace => {
                        presentation.state.base = Some(poor);
                        presentation.state.layer = None;
                    }
                    PoorBgaMode::Overlay => {
                        presentation.poor_overlay = Some(poor);
                    }
                    PoorBgaMode::Off => {}
                }
            }
        }
        Ok(presentation)
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::{
        judge::{JudgeEvent, JudgeOutcome, JudgeStage, MissReason},
        time::Timestamp,
    };
    use beatkernel_bms::{ImageId, ParseOptions};
    use std::sync::Arc;
    fn fixture_chart(visual: &str) -> Arc<PlayerChart> {
        let source = beatkernel_bms::parse(
            &format!("#BPM 60\n#WAV01 head.wav\n#00011:0101\n{visual}"),
            ParseOptions::default(),
        )
        .unwrap();
        Arc::new(PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap())
    }
    fn miss(chart: &PlayerChart, index: usize, at: i64) -> JudgeEvent {
        JudgeEvent {
            object: chart.notes[index].object,
            stage: JudgeStage::Instant,
            outcome: JudgeOutcome::Miss {
                reason: MissReason::HeadTimeout,
            },
            at: Timestamp::from_nanos(at),
            input: None,
        }
    }
    #[test]
    fn exact_boundaries_future_pause_and_fresh_prefix_preserve_original_selections() {
        let chart = fixture_chart("#BMP00 poor.png\n#00004:01\n#00007:02\n#00006:03");
        let mut progress = NoteProgress::new(chart.clone()).unwrap();
        let policy = PoorBackgroundPolicy::default();
        let normal = chart.bga_state(Timestamp::ZERO);
        assert_eq!(normal.poor, Some(ImageId(3)));
        assert_eq!(
            policy
                .project(&chart, Timestamp::ZERO, Some(&progress))
                .unwrap(),
            normal
        );
        progress.apply(&[miss(&chart, 0, 100)]);
        let before = policy
            .project(&chart, Timestamp::from_nanos(99), Some(&progress))
            .unwrap();
        assert_eq!(before, normal);
        let active = policy
            .project(&chart, Timestamp::from_nanos(100), Some(&progress))
            .unwrap();
        assert_eq!(active.base, Some(ImageId(3)));
        assert_eq!(active.layer, None);
        assert_eq!(active.poor, Some(ImageId(3)));
        assert_eq!(
            policy
                .project(&chart, Timestamp::from_nanos(100), Some(&progress))
                .unwrap(),
            active
        );
        assert_eq!(
            policy
                .project(&chart, Timestamp::from_nanos(500_000_099), Some(&progress))
                .unwrap(),
            active
        );
        assert_eq!(
            policy
                .project(&chart, Timestamp::from_nanos(500_000_100), Some(&progress))
                .unwrap(),
            normal
        );
        assert_eq!(
            PoorBackgroundPolicy::new(0)
                .unwrap()
                .project(&chart, Timestamp::from_nanos(100), Some(&progress))
                .unwrap(),
            normal
        );
        let fresh = NoteProgress::new(chart.clone()).unwrap();
        assert_eq!(
            policy
                .project(&chart, Timestamp::from_nanos(100), Some(&fresh))
                .unwrap(),
            normal
        );
        assert_eq!(
            policy
                .project(&chart, Timestamp::from_nanos(100), None)
                .unwrap(),
            normal
        );
    }
    #[test]
    fn extremes_undefined_poor_missing_selection_and_disabled_identity_are_explicit() {
        let policy = PoorBackgroundPolicy::default();
        let chart = fixture_chart("#BMP00 initial.png\n#00006:ZZ");
        let mut progress = NoteProgress::new(chart.clone()).unwrap();
        progress.apply(&[miss(&chart, 0, i64::MIN)]);
        assert_eq!(
            policy
                .project(&chart, Timestamp::from_nanos(i64::MAX), Some(&progress))
                .unwrap(),
            chart.bga_state(Timestamp::from_nanos(i64::MAX))
        );
        progress.apply(&[miss(&chart, 1, i64::MAX)]);
        let at = policy
            .project(&chart, Timestamp::from_nanos(i64::MAX), Some(&progress))
            .unwrap();
        assert_eq!(at.base, Some(ImageId(1295))); // Undefined selections remain explicit.
        let foreign = NoteProgress::new(Arc::new(chart.as_ref().clone())).unwrap();
        assert!(
            PoorBackgroundPolicy::new(0)
                .unwrap()
                .project(&chart, Timestamp::ZERO, Some(&foreign))
                .is_err()
        );
        let missing = fixture_chart("#00004:01\n#00007:02");
        let mut missing_progress = NoteProgress::new(missing.clone()).unwrap();
        missing_progress.apply(&[miss(&missing, 0, 0)]);
        assert_eq!(
            policy
                .project(&missing, Timestamp::ZERO, Some(&missing_progress))
                .unwrap(),
            missing.bga_state(Timestamp::ZERO)
        );
        assert!(PoorBackgroundPolicy::new(-1).is_err());
        assert!(PoorBackgroundPolicy::new(10_000_000_001).is_err());
        assert!(PoorBackgroundPolicy::new(10_000_000_000).is_ok());
    }
    #[test]
    fn active_miss_uses_current_poor_selection_and_ordered_latest_effective_miss() {
        let chart = fixture_chart("#BMP00 initial.png\n#00004:01\n#00007:02\n#00006:0304");
        let mut progress = NoteProgress::new(chart.clone()).unwrap();
        progress.apply(&[miss(&chart, 0, 0)]);
        let policy = PoorBackgroundPolicy::new(10_000_000_000).unwrap();
        assert_eq!(
            policy
                .project(&chart, Timestamp::ZERO, Some(&progress))
                .unwrap()
                .base,
            Some(ImageId(3))
        );
        assert_eq!(
            policy
                .project(
                    &chart,
                    Timestamp::from_nanos(2_000_000_000),
                    Some(&progress)
                )
                .unwrap()
                .base,
            Some(ImageId(4))
        );
        let snapshot = progress.clone();
        progress.apply(&[miss(&chart, 1, 3_000_000_000)]);
        assert_eq!(
            policy
                .project(
                    &chart,
                    Timestamp::from_nanos(2_000_000_000),
                    Some(&progress)
                )
                .unwrap(),
            chart.bga_state(Timestamp::from_nanos(2_000_000_000))
        );
        assert_eq!(
            policy
                .project(
                    &chart,
                    Timestamp::from_nanos(2_000_000_000),
                    Some(&snapshot)
                )
                .unwrap()
                .base,
            Some(ImageId(4))
        );
    }
    #[test]
    fn complete_mode_intent_preserves_normal_layers_for_overlay_and_off() {
        let policy = PoorBackgroundPolicy::default();
        for (digit, mode) in [
            ("0", PoorBgaMode::Replace),
            ("1", PoorBgaMode::Overlay),
            ("2", PoorBgaMode::Off),
        ] {
            let chart = fixture_chart(&format!(
                "#POORBGA {digit}\n#BMP00 initial.png\n#00004:01\n#00007:02\n#00006:03"
            ));
            assert_eq!(chart.poor_bga_mode, mode);
            let mut progress = NoteProgress::new(chart.clone()).unwrap();
            let normal = chart.bga_state(Timestamp::ZERO);
            assert_eq!(
                policy
                    .select(&chart, Timestamp::ZERO, Some(&progress))
                    .unwrap(),
                BgaPresentation::from(normal)
            );
            progress.apply(&[miss(&chart, 0, 100)]);
            assert_eq!(
                policy
                    .select(&chart, Timestamp::from_nanos(99), Some(&progress))
                    .unwrap(),
                normal.into()
            );
            let active = policy
                .select(&chart, Timestamp::from_nanos(100), Some(&progress))
                .unwrap();
            match mode {
                PoorBgaMode::Replace => {
                    assert_eq!(active.state.base, normal.poor);
                    assert_eq!(active.state.layer, None);
                    assert_eq!(active.poor_overlay, None);
                }
                PoorBgaMode::Overlay => {
                    assert_eq!(active.state, normal);
                    assert_eq!(active.poor_overlay, normal.poor);
                }
                PoorBgaMode::Off => assert_eq!(active, normal.into()),
            }
            assert_eq!(
                policy
                    .project(&chart, Timestamp::from_nanos(100), Some(&progress))
                    .unwrap(),
                active.state
            );
            assert_eq!(
                policy
                    .select(&chart, Timestamp::from_nanos(100), Some(&progress))
                    .unwrap(),
                active
            );
            assert_eq!(
                policy
                    .select(&chart, Timestamp::from_nanos(500_000_099), Some(&progress))
                    .unwrap(),
                active
            );
            assert_eq!(
                policy
                    .select(&chart, Timestamp::from_nanos(500_000_100), Some(&progress))
                    .unwrap(),
                normal.into()
            );
            assert_eq!(
                PoorBackgroundPolicy::new(0)
                    .unwrap()
                    .select(&chart, Timestamp::from_nanos(100), Some(&progress))
                    .unwrap(),
                normal.into()
            );
            let foreign = NoteProgress::new(Arc::new(chart.as_ref().clone())).unwrap();
            assert!(
                policy
                    .select(&chart, Timestamp::ZERO, Some(&foreign))
                    .is_err()
            );
        }
    }
    #[test]
    fn overlay_does_not_invent_poor_but_retains_declared_undefined_reference() {
        for visual in [
            "#POORBGA 1\n#00004:01\n#00007:02",
            "#POORBGA 1\n#00004:01\n#00007:02\n#00006:ZZ",
        ] {
            let chart = fixture_chart(visual);
            let mut progress = NoteProgress::new(chart.clone()).unwrap();
            progress.apply(&[miss(&chart, 0, 0)]);
            let selection = PoorBackgroundPolicy::default()
                .select(&chart, Timestamp::ZERO, Some(&progress))
                .unwrap();
            assert_eq!(selection.state, chart.bga_state(Timestamp::ZERO));
            assert_eq!(selection.poor_overlay, selection.state.poor);
        }
    }
}
