// Included beneath the existing actual native-owner Mixer fixture.
use crate::play_policy::{ResolvedPlayPolicy, ClassifiedWindow};
fn resolved(f: &Fixture, kind: beatkernel_bms::BmsGaugeKind) -> ResolvedPlayPolicy {
    ResolvedPlayPolicy::bms(
        &f.source,
        kind,
        &[ClassifiedWindow {
            judgment: beatkernel_bms::BmsJudgment::Bad,
            window: f.runtime.judge().profile().windows()[0],
        }],
        0,
    )
    .unwrap()
}
fn custom_fixture(kind: beatkernel_bms::BmsGaugeKind) -> Fixture {
    let mut f = Fixture::new(false, false);
    let policy = resolved(&f, kind);
    f.gauge = BmsGauge::new(policy.gauge().try_copy().unwrap());
    f.capture = crate::native_judge::prepare_section_capture_for_policy(
        &f.source,
        f.runtime.judge(),
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
fn config() -> NativeGameplayConfig {
    NativeGameplayConfig {
        origin: point(1, 0),
        stream_origin: point(2, 0),
        playback_origin: point(2, 0),
        song_origin: Timestamp::ZERO,
        sample_rate: 1000,
        end_song: None,
        advance_lag: Duration::from_nanos(10_000_000),
        seconds: None,
        pause_supported: false,
        logical_schedule: true,
    }
}
#[test]
fn actual_native_solo_pump_preserves_each_nondefault_gauge_and_recorded_policy() {
    for kind in beatkernel_bms::BmsGaugeKind::ALL {
        let mut f = custom_fixture(kind);
        let initial = f.gauge.profile().try_copy().unwrap();
        let (publisher, viewer) = player::channel();
        player::with_publisher(publisher, || {
            player::publish_chart(&f.source, f.runtime.judge().chart()).unwrap();
            f.run(false, true).unwrap();
            player::publish_pause(player::PauseState::Paused);
            player::publish_pause(player::PauseState::Running);
            let snapshot = viewer.take_latest().unwrap();
            assert_eq!(snapshot.gauge, f.gauge);
            Ok(())
        })
        .unwrap();
        assert!(f.device.step > 0);
        assert_eq!(f.gauge.profile(), &initial);
        let file = f.capture.take().unwrap().into_file();
        assert_eq!(
            crate::replay_playback::decode_section_setup(&file.header.options)
                .unwrap()
                .gauge,
            initial
        );
        let mut replay =
            crate::replay_visual::ReplayVisual::new_section(&f.source, &file, limits()).unwrap();
        replay
            .advance_to(file.records.last().unwrap().song_time)
            .unwrap();
        assert_eq!(replay.gauge(), &f.gauge);
    }
}
#[test]
fn nondefault_native_setup_refusals_precede_device_or_input_effects() {
    let mut f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    f.capture =
        Some(LiveReplayCapture::new(f.runtime.judge(), ClockDomainId(1), limits()).unwrap());
    let before = f.gauge.clone();
    assert!(f.run(false, true).is_err());
    assert_eq!(f.device.step, 0);
    assert!(f.runtime.judge().effective_song_time().is_none());
    assert_eq!(f.gauge, before);
    assert!(f.capture.as_ref().unwrap().records().is_empty());
    let mut f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    f.runtime
        .advance_to(point(1, 0), &ExplicitDomains, point(2, 0))
        .unwrap();
    assert!(f.run(false, true).is_err());
    assert_eq!(f.device.step, 0);
    let mut f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    assert!(
        crate::native_policy_admission::validate_header(
            f.runtime.judge(),
            f.gauge.profile(),
            f.capture.as_ref().unwrap().header(),
            &NativeGameplayConfig {
                end_song: Some(Timestamp::from_nanos(1)),
                ..config()
            }
        )
        .is_err()
    );
    let mut header = f.capture.as_ref().unwrap().header().clone();
    header.chart_identity[0] ^= 1;
    assert!(
        crate::native_policy_admission::validate_header(
            f.runtime.judge(),
            f.gauge.profile(),
            &header,
            &config()
        )
        .is_err()
    );
    header = f.capture.as_ref().unwrap().header().clone();
    header.normalized_clock = ClockDomainId(99);
    assert!(
        crate::native_policy_admission::validate_header(
            f.runtime.judge(),
            f.gauge.profile(),
            &header,
            &config()
        )
        .is_err()
    );
    f.capture = None;
    f.run(false, true).unwrap();
    assert!(f.device.step > 0);
}
#[test]
fn canonical_header_checks_preroll_start_seed_and_all_original_identity_bytes() {
    let f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    let cfg = NativeGameplayConfig {
        song_origin: Timestamp::from_nanos(-3),
        playback_origin: point(2, 8),
        ..config()
    };
    let header = crate::replay_capture::setup_gauge_header(
        f.runtime.judge(),
        ClockDomainId(1),
        limits(),
        Timestamp::from_nanos(5),
        u64::MAX,
        None,
        beatkernel_bms::BmsInputMode::ButtonOnly,
        None,
        f.gauge.profile(),
    )
    .unwrap();
    crate::native_policy_admission::validate_header(
        f.runtime.judge(),
        f.gauge.profile(),
        &header,
        &cfg,
    )
    .unwrap();
    let mut wrong = header.clone();
    wrong.chart_identity.push(0);
    assert!(
        crate::native_policy_admission::validate_header(
            f.runtime.judge(),
            f.gauge.profile(),
            &wrong,
            &cfg
        )
        .is_err()
    );
    wrong = header.clone();
    wrong.chart_identity["bms-judge-setup/v1:".len()] ^= 1;
    assert!(
        crate::native_policy_admission::validate_header(
            f.runtime.judge(),
            f.gauge.profile(),
            &wrong,
            &cfg
        )
        .is_err()
    );
    wrong = header;
    wrong.options.push(0);
    assert!(
        crate::native_policy_admission::validate_header(
            f.runtime.judge(),
            f.gauge.profile(),
            &wrong,
            &cfg
        )
        .is_err()
    );
}

struct PolicyCompetition {
    header: Option<beatkernel::replay::ReplayHeader>,
    observed: usize,
}
impl crate::gameplay_competition::SoloCompetitionPort for PolicyCompetition {
    fn expected_policy_header(&self) -> Option<&beatkernel::replay::ReplayHeader> {
        self.header.as_ref()
    }
    fn observe(&mut self, _: &RuntimeReport) -> NativeGameplayResult<()> {
        self.observed += 1;
        Ok(())
    }
    fn mark_native_completed(&mut self) {}
}
#[derive(Default)]
struct Control {
    reads: usize,
    waits: usize,
}
impl crate::native_pump_control::NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        self.reads += 1;
        Ok(0)
    }
    fn checked_add(moment: u64, duration: WallDuration) -> Option<u64> {
        moment.checked_add(duration.as_nanos().try_into().ok()?)
    }
    fn wait(&mut self, _: WallDuration) -> NativeGameplayResult<()> {
        self.waits += 1;
        Ok(())
    }
}
fn run_competition<S: crate::gameplay_competition::SoloCompetitionPort>(
    f: &mut Fixture,
    competition: &mut Option<S>,
    control: &mut Control,
    cfg: NativeGameplayConfig,
) -> NativeGameplayResult<()> {
    run_gameplay_with_ports(
        &mut f.device,
        GameplaySession {
            runtime: &mut f.runtime,
            gauge: &mut f.gauge,
            bgm: &mut f.bgm,
            discipline: &mut f.discipline,
            pause: &mut f.pause,
            end: &mut f.end,
            completion: &mut f.completion,
            capture: &mut f.capture,
            competition,
            delivery: &mut f.delivery,
            pre_origin_inputs: &mut f.pre,
        },
        cfg,
        control,
        &mut crate::native_gameplay_host::NoopGameplayHost,
    )
}
#[test]
fn competition_identity_and_capture_agree_before_control_or_native_effects() {
    let mut f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    let header = f.capture.as_ref().unwrap().header().clone();
    let mut port = Some(PolicyCompetition {
        header: Some(header),
        observed: 0,
    });
    let mut control = Control::default();
    run_competition(&mut f, &mut port, &mut control, config()).unwrap();
    assert!(port.as_ref().unwrap().observed > 0);
    assert!(f.device.step > 0);
    for opaque in [true, false] {
        let mut f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
        let wrong = LiveReplayCapture::new(f.runtime.judge(), ClockDomainId(1), limits())
            .unwrap()
            .header()
            .clone();
        let mut port = Some(PolicyCompetition {
            header: (!opaque).then_some(wrong),
            observed: 0,
        });
        let mut control = Control::default();
        assert!(
            run_competition(
                &mut f,
                &mut port,
                &mut control,
                NativeGameplayConfig {
                    seconds: Some(1),
                    ..config()
                }
            )
            .is_err()
        );
        assert_eq!(f.device.step, 0);
        assert_eq!(control.reads, 0);
        assert_eq!(port.unwrap().observed, 0);
    }
    let mut f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    let mut header = f.capture.as_ref().unwrap().header().clone();
    let hash = f.runtime.judge().stable_hash().unwrap();
    header.chart_identity = b"bms-judge-setup/v2:".to_vec();
    header.chart_identity.extend_from_slice(&hash.to_le_bytes());
    header.chart_identity.extend_from_slice(&7u64.to_le_bytes());
    let mut port = Some(PolicyCompetition {
        header: Some(header),
        observed: 0,
    });
    let mut control = Control::default();
    assert!(run_competition(&mut f, &mut port, &mut control, config()).is_err());
    assert_eq!(f.device.step, 0);
    let mut f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    let mut noop = Some(crate::gameplay_competition::NoopSoloCompetition);
    let mut control = Control::default();
    run_competition(&mut f, &mut noop, &mut control, config()).unwrap();
    assert!(f.device.step > 0);
}

#[test]
fn poisoned_solo_with_pristine_judge_refuses_before_host_control_and_device() {
    let mut f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    assert!(
        f.runtime
            .advance_to(point(99, 0), &ExplicitDomains, point(2, 0))
            .is_err()
    );
    assert!(f.runtime.poisoned());
    assert!(f.runtime.judge().effective_song_time().is_none());
    assert!(f.runtime.gameplay_fence().is_none());
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_chart(&f.source, f.runtime.judge().chart()).unwrap();
        let initial = viewer.take_latest().unwrap();
        let mut control = Control::default();
        let result = run_gameplay_with_ports(
            &mut f.device,
            GameplaySession {
                runtime: &mut f.runtime,
                gauge: &mut f.gauge,
                bgm: &mut f.bgm,
                discipline: &mut f.discipline,
                pause: &mut f.pause,
                end: &mut f.end,
                completion: &mut f.completion,
                capture: &mut f.capture,
                competition: &mut f.competition,
                delivery: &mut f.delivery,
                pre_origin_inputs: &mut f.pre,
            },
            NativeGameplayConfig {
                seconds: Some(1),
                ..config()
            },
            &mut control,
            &mut crate::native_gameplay_bridge::PlayerGameplayHost,
        );
        assert!(result.is_err());
        assert_eq!(control.reads, 0);
        assert_eq!(f.device.step, 0);
        assert!(viewer.take_latest().is_none());
        player::publish_pause(player::PauseState::Paused);
        player::publish_pause(player::PauseState::Running);
        let after = viewer.take_latest().unwrap();
        assert_eq!(initial.gauge, after.gauge);
        assert_eq!(after.gauge, BmsGauge::default());
        assert!(after.players[0].song_time.is_none());
        Ok(())
    })
    .unwrap();
}
