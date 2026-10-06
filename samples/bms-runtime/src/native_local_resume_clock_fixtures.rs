use super::*;
fn replay_limits() -> beatkernel::replay::codec::ReplayCodecLimits {
    beatkernel::replay::codec::ReplayCodecLimits::new(
        65536,
        128,
        4096,
        beatkernel::input::CodecLimits::new(4096, 4096).unwrap(),
    )
    .unwrap()
}
#[test]
fn actual_cohort_resume_preserves_shared_epoch_settings_and_each_original_player_capture_without_completion()
 {
    for epoch in [17, u64::MAX] {
        let mut f = Fixture::new(false);
        let settings = DisciplineConfig {
            capacity: 8,
            min_span: Duration::from_nanos(500_000_000),
            correction_horizon: Duration::from_nanos(9_000_000_000),
            ..Default::default()
        };
        f.presentation =
            PresentationEstimator::new(settings, point(2, 0), ClockDomainId(1), Timestamp::ZERO)
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
                    replay_limits(),
                )
                .unwrap(),
            );
        }
        let command = std::rc::Rc::new(std::cell::Cell::new(false));
        f.device.pause_command = Some(command.clone());
        let mut host = Host {
            pause_command: Some(command),
            ..Default::default()
        };
        run_cohort_with_ports(
            &mut f.device,
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
        .unwrap();
        assert_eq!(f.presentation.epoch(), epoch);
        assert_eq!(f.presentation.config(), settings);
        assert_eq!(f.device.seeds, [pair(40_000_000, 40_000_000)]);
        assert_eq!(f.pause.phase(), crate::playback_pause::PausePhase::Running);
        assert_eq!(f.delivery.observed_events(), 8);
        assert_eq!(f.network.marks, 0);
        assert_eq!(
            f.states.iter().map(|s| s.player).collect::<Vec<_>>(),
            [PlayerId(7), PlayerId(u32::MAX)]
        );
        for state in &f.states {
            assert_eq!(state.competition.as_ref().unwrap().marks, 0);
            let capture = state.capture.as_ref().unwrap();
            let releases = capture
                .records()
                .iter()
                .filter_map(|r| match &r.operation {
                    beatkernel::replay::ReplayOperation::Input(input) => match &input.physical {
                        PhysicalInputEvent::Button(button) if button.state == ButtonState::Up => {
                            Some((button.meta.timestamp, button.meta.original_clock_point))
                        }
                        _ => None,
                    },
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                releases,
                [
                    (
                        Timestamp::from_nanos(40_000_000),
                        Some(point(1, 20_000_000))
                    ),
                    (Timestamp::from_nanos(45_000_000), None)
                ]
            );
            crate::replay_playback::reconstruct(
                &source(),
                beatkernel::replay::codec::ReplayFile::new(
                    capture.header().clone(),
                    capture.records().to_vec(),
                ),
                replay_limits(),
            )
            .unwrap();
        }
        assert!(
            f.presentation
                .observe_clock_pair_in_epoch(0, pair(80_000_000, 80_000_000))
                .is_err()
        );
        assert!(host.pause_states.contains(&PauseState::Resuming));
    }
}
