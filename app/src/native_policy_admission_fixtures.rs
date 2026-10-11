// Included beneath the existing actual native-owner Mixer fixture.
use crate::live_speed::LiveSpeedSetup;
use crate::native_policy_admission as speed_admission;
use crate::play_policy::{ResolvedPlayPolicy, ClassifiedWindow};

fn speed_setup(start: i64, preroll: i64, fraction: &str, end: Option<i64>) -> LiveSpeedSetup {
    LiveSpeedSetup::new(
        Timestamp::from_nanos(start),
        Duration::from_nanos(preroll),
        fraction.parse().unwrap(),
        end.map(Timestamp::from_nanos),
    )
    .unwrap()
}
fn speed_config(setup: LiveSpeedSetup) -> NativeGameplayConfig {
    NativeGameplayConfig {
        song_origin: setup.song_origin(),
        playback_origin: point(2, setup.preroll().as_nanos()),
        end_song: setup.end(),
        ..config()
    }
}
fn speed_capture(
    f: &Fixture,
    policy: &ResolvedPlayPolicy,
    setup: LiveSpeedSetup,
) -> LiveReplayCapture {
    LiveReplayCapture::new_with_policy_and_speed(
        f.runtime.judge(),
        ClockDomainId(1),
        limits(),
        setup.start(),
        0,
        setup.end(),
        beatkernel_bms::BmsInputMode::ButtonOnly,
        None,
        policy,
        setup.speed(),
    )
    .unwrap()
}
fn speed_selected(
    f: &Fixture,
    policy: &ResolvedPlayPolicy,
    capture: Option<&LiveReplayCapture>,
    opponent: Option<&beatkernel::replay::ReplayHeader>,
    cfg: &NativeGameplayConfig,
    logical_domain: ClockDomainId,
    setup: LiveSpeedSetup,
) -> NativeGameplayResult<()> {
    speed_admission::validate_selected_in_domain_with_speed(
        f.runtime.judge(),
        &f.gauge,
        policy,
        capture,
        opponent,
        cfg,
        logical_domain,
        Some(setup.start()),
        setup,
    )
}

#[test]
fn selected_live_speed_admits_literal_four_rates_with_original_sections() {
    for (fraction, origin) in [("1/2", 9), ("1/1", 7), ("2/1", 4), ("3/2", 6)] {
        let f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
        let policy = resolved(&f, beatkernel_bms::BmsGaugeKind::Hard);
        let setup = speed_setup(10, 3, fraction, Some(100));
        assert_eq!(setup.song_origin(), Timestamp::from_nanos(origin));
        let capture = speed_capture(&f, &policy, setup);
        let cfg = speed_config(setup);
        speed_selected(
            &f,
            &policy,
            Some(&capture),
            Some(capture.header()),
            &cfg,
            ClockDomainId(1),
            setup,
        )
        .unwrap();
        speed_admission::validate_audio_config_with_speed(
            &AudioGameplayConfig {
                gameplay: cfg,
                section_start: setup.start(),
            },
            setup,
        )
        .unwrap();
        let transport = setup.transport(cfg.origin.timestamp).unwrap();
        speed_admission::validate_transport_with_speed(&transport, cfg.origin.timestamp, setup)
            .unwrap();
        assert!(capture.records().is_empty());
        assert!(f.runtime.judge().effective_song_time().is_none());
        assert_eq!(f.device.step, 0);
    }
}

#[test]
fn selected_normalized_fraction_matches_capture_and_competition_identity() {
    let f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    let policy = resolved(&f, beatkernel_bms::BmsGaugeKind::Hard);
    let original = speed_setup(5, 1, "3/2", Some(100));
    let equivalent = speed_setup(5, 1, "6/4", Some(100));
    let capture = speed_capture(&f, &policy, original);
    let other = speed_capture(&f, &policy, equivalent);
    assert_eq!(capture.header(), other.header());
    speed_selected(
        &f,
        &policy,
        Some(&capture),
        Some(other.header()),
        &speed_config(equivalent),
        ClockDomainId(1),
        equivalent,
    )
    .unwrap();
}

#[test]
fn selected_speed_refuses_wrong_source_start_end_and_domain_atomically() {
    let f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    let policy = resolved(&f, beatkernel_bms::BmsGaugeKind::Hard);
    let selected = speed_setup(10, 3, "3/2", Some(100));
    let capture = speed_capture(&f, &policy, selected);
    let before_gauge = f.gauge.clone();
    let before_header = capture.header().clone();
    for wrong in [
        speed_setup(10, 3, "1/2", Some(100)),
        speed_setup(11, 3, "3/2", Some(100)),
        speed_setup(10, 3, "3/2", Some(101)),
    ] {
        let competitor = speed_capture(&f, &policy, wrong);
        assert!(speed_selected(
            &f,
            &policy,
            Some(&capture),
            Some(competitor.header()),
            &speed_config(selected),
            ClockDomainId(1),
            selected
        )
        .is_err());
        assert_eq!(capture.header(), &before_header);
        assert!(capture.records().is_empty());
        assert_eq!(f.gauge, before_gauge);
        assert!(f.runtime.judge().effective_song_time().is_none());
        assert_eq!(f.device.step, 0);
    }
    assert!(speed_selected(
        &f,
        &policy,
        Some(&capture),
        None,
        &speed_config(selected),
        ClockDomainId(99),
        selected
    )
    .is_err());
}

#[test]
fn explicit_speed_header_uses_original_start_not_normal_preroll_inference() {
    let f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    let policy = resolved(&f, beatkernel_bms::BmsGaugeKind::Hard);
    let setup = speed_setup(10, 3, "3/2", Some(100));
    let capture = speed_capture(&f, &policy, setup);
    // Integer origin 6 plus physical P=3 would incorrectly infer start=9.
    let cfg = speed_config(setup);
    speed_admission::validate_header_in_domain_with_speed(
        f.runtime.judge(),
        f.gauge.profile(),
        capture.header(),
        &cfg,
        ClockDomainId(1),
        None,
        setup,
    )
    .unwrap();
    assert!(speed_admission::validate_header(
        f.runtime.judge(),
        f.gauge.profile(),
        capture.header(),
        &cfg
    )
    .is_err());
}

#[test]
fn exact_transport_admission_rejects_same_integer_anchor_different_phase() {
    for (selected, impostor) in [
        (
            speed_setup(0, 1, "1/2", None),
            speed_setup(0, 0, "1/2", None),
        ),
        (
            speed_setup(0, 3, "3/2", None),
            speed_setup(-1, 2, "3/2", None),
        ),
    ] {
        let actual = selected.transport(Timestamp::from_nanos(17)).unwrap();
        let wrong = impostor.transport(Timestamp::from_nanos(17)).unwrap();
        assert_eq!(actual.anchor(), wrong.anchor());
        speed_admission::validate_transport_with_speed(
            &actual,
            Timestamp::from_nanos(17),
            selected,
        )
        .unwrap();
        assert!(speed_admission::validate_transport_with_speed(
            &wrong,
            Timestamp::from_nanos(17),
            selected
        )
        .is_err());
        let legacy = Transport::new(
            Timestamp::from_nanos(17),
            selected.song_origin(),
            selected.speed().rate(),
        );
        assert!(speed_admission::validate_transport_with_speed(
            &legacy,
            Timestamp::from_nanos(17),
            selected
        )
        .is_err());
    }
}

#[test]
fn exact_transport_refuses_wrong_origin_end_rate_and_modified_history() {
    let setup = speed_setup(10, 3, "3/2", Some(100));
    let origin = Timestamp::from_nanos(17);
    for wrong in [
        speed_setup(10, 3, "3/2", Some(101)),
        speed_setup(10, 3, "2/1", Some(100)),
    ] {
        assert!(speed_admission::validate_transport_with_speed(
            &wrong.transport(origin).unwrap(),
            origin,
            setup
        )
        .is_err());
    }
    let mut transport = setup.transport(origin).unwrap();
    assert!(speed_admission::validate_transport_with_speed(
        &transport,
        Timestamp::from_nanos(18),
        setup
    )
    .is_err());
    transport
        .pause_at_output_elapsed(Timestamp::from_nanos(18), 1)
        .unwrap();
    assert!(speed_admission::validate_transport_with_speed(&transport, origin, setup).is_err());
    transport.resume_preroll(Timestamp::from_nanos(19)).unwrap();
    assert!(speed_admission::validate_transport_with_speed(&transport, origin, setup).is_err());
    let mut noop = setup.transport(origin).unwrap();
    noop.set_rate(Timestamp::from_nanos(18), setup.speed().rate())
        .unwrap();
    assert_eq!(noop.anchors().len(), 1);
    assert!(speed_admission::validate_transport_with_speed(&noop, origin, setup).is_err());
    let mut resumed = setup.transport(origin).unwrap();
    resumed.resume_preroll(Timestamp::from_nanos(18)).unwrap();
    assert_eq!(resumed.anchors().len(), 1);
    assert!(speed_admission::validate_transport_with_speed(&resumed, origin, setup).is_err());
}

#[test]
fn selected_speed_rejects_noncanonical_header_and_different_selected_policy() {
    let f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    let policy = resolved(&f, beatkernel_bms::BmsGaugeKind::Hard);
    let setup = speed_setup(10, 3, "3/2", Some(100));
    let capture = speed_capture(&f, &policy, setup);
    for which in 0..5 {
        let mut header = capture.header().clone();
        match which {
            0 => header.options.push(0),
            1 => header.chart_identity[0] ^= 1,
            2 => header.rules_identity.push(0),
            3 => header.seed = 1,
            _ => header.version = u32::MAX,
        }
        assert!(speed_selected(
            &f,
            &policy,
            None,
            Some(&header),
            &speed_config(setup),
            ClockDomainId(1),
            setup
        )
        .is_err());
    }
    let different = resolved(&f, beatkernel_bms::BmsGaugeKind::Easy);
    assert!(speed_selected(
        &f,
        &different,
        Some(&capture),
        None,
        &speed_config(setup),
        ClockDomainId(1),
        setup
    )
    .is_err());
    assert!(capture.records().is_empty());
    assert_eq!(f.device.step, 0);
}

#[test]
fn selected_speed_rejects_capture_already_containing_original_operations() {
    let f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    let policy = resolved(&f, beatkernel_bms::BmsGaugeKind::Hard);
    let setup = speed_setup(0, 0, "3/2", None);
    let mut capture = speed_capture(&f, &policy, setup);
    let mut processed = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    let report = processed
        .runtime
        .advance_to(point(1, 0), &ExplicitDomains, point(2, 0))
        .unwrap();
    capture.record_report(&report).unwrap();
    assert!(!capture.records().is_empty());
    let before = capture.records().len();
    assert!(speed_selected(
        &f,
        &policy,
        Some(&capture),
        None,
        &speed_config(setup),
        ClockDomainId(1),
        setup
    )
    .is_err());
    assert_eq!(capture.records().len(), before);
    assert!(f.runtime.judge().effective_song_time().is_none());
    assert_eq!(f.device.step, 0);
}

#[test]
fn explicit_native_setup_mismatches_refuse_even_without_capture() {
    let f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    let policy = resolved(&f, beatkernel_bms::BmsGaugeKind::Hard);
    let setup = speed_setup(10, 3, "3/2", Some(100));
    let cfg = speed_config(setup);
    for wrong in [
        NativeGameplayConfig {
            song_origin: Timestamp::from_nanos(7),
            ..cfg
        },
        NativeGameplayConfig {
            end_song: Some(Timestamp::from_nanos(101)),
            ..cfg
        },
        NativeGameplayConfig {
            sample_rate: 0,
            ..cfg
        },
        NativeGameplayConfig {
            stream_origin: cfg.origin,
            ..cfg
        },
    ] {
        assert!(speed_selected(&f, &policy, None, None, &wrong, ClockDomainId(1), setup).is_err());
    }
    assert!(speed_admission::validate_audio_config_with_speed(
        &AudioGameplayConfig {
            gameplay: cfg,
            section_start: Timestamp::from_nanos(9)
        },
        setup
    )
    .is_err());
}

#[test]
fn nonpristine_selected_judge_is_refused_without_mutating_capture() {
    let mut f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    let policy = resolved(&f, beatkernel_bms::BmsGaugeKind::Hard);
    let setup = speed_setup(0, 1, "1/2", None);
    let capture = speed_capture(&f, &policy, setup);
    f.runtime
        .advance_to(point(1, 0), &ExplicitDomains, point(2, 0))
        .unwrap();
    let before = f.runtime.judge().stable_hash().unwrap();
    assert!(speed_selected(
        &f,
        &policy,
        Some(&capture),
        None,
        &speed_config(setup),
        ClockDomainId(1),
        setup
    )
    .is_err());
    assert_eq!(f.runtime.judge().stable_hash().unwrap(), before);
    assert!(capture.records().is_empty());
    assert_eq!(f.device.step, 0);
}

#[test]
fn nonpristine_selected_gauge_refuses_with_pristine_judge_and_capture() {
    let mut f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    let policy = resolved(&f, beatkernel_bms::BmsGaugeKind::Hard);
    let setup = speed_setup(0, 1, "1/2", None);
    let capture = speed_capture(&f, &policy, setup);
    f.gauge
        .observe(
            &[],
            &[beatkernel::judge::HazardEvent {
                id: beatkernel::judge::HazardId(1),
                at: Timestamp::ZERO,
                control: GameControlId(0x11),
                value: 1295,
                outcome: beatkernel::judge::HazardOutcome::Triggered,
                input: None,
            }],
        )
        .unwrap();
    let before = f.gauge.clone();
    assert!(f.runtime.judge().effective_song_time().is_none());
    assert!(speed_selected(
        &f,
        &policy,
        Some(&capture),
        None,
        &speed_config(setup),
        ClockDomainId(1),
        setup
    )
    .is_err());
    assert_eq!(f.gauge, before);
    assert!(capture.records().is_empty());
    assert_eq!(f.device.step, 0);
}

#[test]
fn wide_original_section_and_physical_origins_do_not_overflow_normal_inference() {
    let f = custom_fixture(beatkernel_bms::BmsGaugeKind::Hard);
    let policy = resolved(&f, beatkernel_bms::BmsGaugeKind::Hard);
    let setup = speed_setup(i64::MAX - 1, 1, "1/2", Some(i64::MAX));
    let cfg = NativeGameplayConfig {
        stream_origin: point(2, i64::MIN),
        playback_origin: point(2, i64::MIN + 1),
        ..speed_config(setup)
    };
    let capture = speed_capture(&f, &policy, setup);
    speed_selected(
        &f,
        &policy,
        Some(&capture),
        None,
        &cfg,
        ClockDomainId(1),
        setup,
    )
    .unwrap();
    speed_admission::validate_audio_config_with_speed(
        &AudioGameplayConfig {
            gameplay: cfg,
            section_start: setup.start(),
        },
        setup,
    )
    .unwrap();
}
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
