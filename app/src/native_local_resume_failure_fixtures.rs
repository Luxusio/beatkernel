use super::*;
use crate::gameplay_resume_failure_fixtures::{
    FaultDevice, SeedMode, TraceHost, settings, limits, assert_original_error,
    assert_paused_history, assert_old_presentation, assert_capture_prefix,
};
fn exercise(mode: SeedMode) {
    for epoch in [13, u64::MAX] {
        let mut f = Fixture::new(false);
        f.presentation =
            PresentationEstimator::new(settings(), point(2, 0), ClockDomainId(1), Timestamp::ZERO)
                .unwrap();
        f.presentation
            .rebind_output(epoch, point(2, 0), point(2, 0), Timestamp::ZERO)
            .unwrap();
        f.presentation
            .observe_clock_pair_in_epoch(epoch, pair(0, 0))
            .unwrap();
        for state in &mut f.states {
            state.capture = Some(
                crate::replay_capture::LiveReplayCapture::new(
                    &judge(&source()),
                    ClockDomainId(1),
                    limits(),
                )
                .unwrap(),
            );
        }
        let command = std::rc::Rc::new(std::cell::Cell::new(false));
        f.device.pause_command = Some(command.clone());
        let mut device = FaultDevice::new(f.device, mode);
        let mut host = TraceHost {
            inner: Host {
                pause_command: Some(command),
                ..Default::default()
            },
            boundaries: Vec::new(),
        };
        let error = run_cohort_with_ports(
            &mut device,
            CohortSession {
                group: &mut f.group,
                network: Some(&mut f.network),
                states: &mut f.states,
                merger: &mut f.merger,
                bgm: &mut f.bgm,
                discipline: &mut f.presentation,
                pause: &mut f.pause,
                end: &mut f.end,
                delivery: &mut f.delivery,
                pre_origin_inputs: &mut f.pre,
            },
            NativeGameplayConfig {
                pause_supported: true,
                ..config(false)
            },
            &mut Control::default(),
            &mut host,
        )
        .unwrap_err();
        assert_original_error(mode, error.as_ref(), &device);
        assert_paused_history(f.group.transport(), &host);
        assert_old_presentation(&f.presentation, &device, epoch);
        let paused = host.boundaries.iter().find(|b| b.paused).unwrap();
        assert_eq!(
            f.states.iter().map(|s| s.player).collect::<Vec<_>>(),
            [PlayerId(7), PlayerId(u32::MAX)]
        );
        for state in &f.states {
            assert_eq!(state.last_song, paused.song);
            assert!(state.completion.is_none());
            assert_eq!(state.competition.as_ref().unwrap().marks, 0);
            assert_capture_prefix(state.capture.as_ref().unwrap(), &host);
        }
        assert_eq!(f.network.marks, 0);
        assert!(host.inner.ends.is_empty());
        assert!(
            host.inner
                .local
                .iter()
                .flat_map(|r| r.iter())
                .all(|(_, song, _)| *song <= paused.song.as_nanos())
        );
        assert!(device.inner.step > 0);
        assert_eq!(device.inner.pcm.len(), device.inner.step * 10);
        assert_eq!(
            device.inner.mixer.frame_cursor(),
            device.inner.pcm.len() as u64
        );
        assert_eq!(f.pause.phase(), crate::playback_pause::PausePhase::Running);
    }
}
#[test]
fn genuine_temporary_seed_then_original_failure_preserves_all_cohort_paused_clocks_and_captures() {
    exercise(SeedMode::FailAfterAdmission);
}
#[test]
fn seed_success_without_evidence_cannot_commit_cohort_resume_cutoffs_or_reports() {
    exercise(SeedMode::NoObservation);
}
