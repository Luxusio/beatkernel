// Uses the existing native cohort device, real RuntimeGroup and actual Mixer.
fn mixed_fixture() -> Fixture {
    let mut f = Fixture::new(false, 8);
    let player = f.states[1].player;
    let judge = f.group.member_judge(player).unwrap();
    let policy = crate::play_policy::ResolvedPlayPolicy::bms(
        &f.source,
        beatkernel_bms::BmsGaugeKind::Hazard,
        &[crate::play_policy::ClassifiedWindow {
            judgment: beatkernel_bms::BmsJudgment::Bad,
            window: judge.profile().windows()[0],
        }],
        0,
    )
    .unwrap();
    f.states[1].gauge = BmsGauge::new(policy.gauge().try_copy().unwrap());
    f.states[1].capture = crate::native_judge::prepare_section_capture_for_policy(
        &f.source,
        judge,
        &policy,
        ClockDomainId(1),
        Timestamp::ZERO,
        0,
        None,
        Some(limits()),
    )
    .unwrap();
    f
}
#[test]
fn actual_mixed_native_cohort_admits_both_profiles_and_preserves_each_capture() {
    let mut f = mixed_fixture();
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        f.register_presentation().unwrap();
        f.run(false).unwrap();
        player::publish_pause(player::PauseState::Paused);
        player::publish_pause(player::PauseState::Running);
        let snapshot = viewer.take_latest().unwrap();
        for state in &f.states {
            let row = snapshot
                .players
                .iter()
                .find(|row| row.player == state.player)
                .unwrap();
            assert_eq!(row.gauge, state.gauge);
            let capture = state.capture.as_ref().unwrap();
            let file = beatkernel::replay::codec::ReplayFile::new(
                capture.header().clone(),
                capture.records().to_vec(),
            );
            let mut replay =
                crate::replay_visual::ReplayVisual::new_section(&f.source, &file, limits())
                    .unwrap();
            replay
                .advance_to(file.records.last().unwrap().song_time)
                .unwrap();
            assert_eq!(replay.gauge(), &state.gauge);
        }
        assert!(f.device.step > 0);
        assert_eq!(f.states[0].gauge.profile(), &GaugeProfile::default());
        assert!(f.states[1].gauge.snapshot().failure.is_some());
        Ok(())
    })
    .unwrap();
}
#[test]
fn late_capture_mismatch_and_group_roster_mismatch_refuse_before_any_cohort_effects() {
    let mut f = mixed_fixture();
    let player = f.states[1].player;
    f.states[1].capture = Some(
        LiveReplayCapture::new(
            f.group.member_judge(player).unwrap(),
            ClockDomainId(1),
            limits(),
        )
        .unwrap(),
    );
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        f.register_presentation().unwrap();
        assert!(f.run(false).is_err());
        assert_eq!(f.device.step, 0);
        player::publish_pause(player::PauseState::Paused);
        player::publish_pause(player::PauseState::Running);
        let snapshot = viewer.take_latest().unwrap();
        assert!(
            snapshot
                .players
                .iter()
                .all(|row| row.gauge == BmsGauge::default() && row.song_time.is_none())
        );
        Ok(())
    })
    .unwrap();
    let mut f = mixed_fixture();
    f.states.swap(0, 1);
    assert!(f.run(false).is_err());
    assert_eq!(f.device.step, 0);
}
