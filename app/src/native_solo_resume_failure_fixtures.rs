use super::*;
use crate::gameplay_resume_failure_fixtures::{
    FaultDevice, SeedMode, TraceHost, settings, limits, assert_original_error,
    assert_paused_history, assert_old_presentation, assert_capture_prefix,
};
fn exercise(mode: SeedMode) {
    for epoch in [7, u64::MAX] {
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
        f.capture = Some(
            crate::replay_capture::LiveReplayCapture::new(
                &judge(&source()),
                ClockDomainId(1),
                limits(),
            )
            .unwrap(),
        );
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
        let error = run_gameplay_with_ports(
            &mut device,
            GameplaySession {
                runtime: &mut f.runtime,
                gauge: &mut f.gauge,
                bgm: &mut f.bgm,
                discipline: &mut f.presentation,
                pause: &mut f.pause,
                end: &mut f.end,
                completion: &mut f.completion,
                capture: &mut f.capture,
                competition: &mut f.observer,
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
        assert_paused_history(f.runtime.transport_mut(), &host);
        assert_old_presentation(&f.presentation, &device, epoch);
        assert_capture_prefix(f.capture.as_ref().unwrap(), &host);
        assert_eq!(f.observer.as_ref().unwrap().marks, 0);
        assert!(f.completion.is_none());
        assert!(host.inner.ends.is_empty());
        assert!(!host.inner.solo.iter().any(
            |r| matches!(&r.input,Some(PhysicalInputEvent::Button(b)) if b.state==ButtonState::Up)
        ));
        assert!(device.inner.step > 0);
        assert_eq!(device.inner.pcm.len(), device.inner.step * 10);
        assert_eq!(
            device.inner.mixer.frame_cursor(),
            device.inner.pcm.len() as u64
        );
        // Physical pause acknowledgement already advanced; only software clocks
        // remain paused. These fixtures deliberately do not assert device undo.
        assert_eq!(f.pause.phase(), crate::playback_pause::PausePhase::Running);
    }
}
#[test]
fn genuine_temporary_seed_then_original_failure_preserves_solo_paused_clocks_and_prefix() {
    exercise(SeedMode::FailAfterAdmission);
}
#[test]
fn seed_success_without_evidence_cannot_publish_solo_resumed_clocks_or_release() {
    exercise(SeedMode::NoObservation);
}
