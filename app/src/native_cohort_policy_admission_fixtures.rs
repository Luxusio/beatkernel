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
        assert!(snapshot
            .players
            .iter()
            .all(|row| row.gauge == BmsGauge::default() && row.song_time.is_none()));
        Ok(())
    })
    .unwrap();
    let mut f = mixed_fixture();
    f.states.swap(0, 1);
    assert!(f.run(false).is_err());
    assert_eq!(f.device.step, 0);
}

mod selected_setup_integration {
    use crate::{
        competition::OpponentKind,
        competition_live::{CompetitionOptions, NetworkRole},
        local_players::PlayerId,
        native_cohort_setup::{
            prepare_audio_cohort_with_policy, prepare_cohort_with_policy, CohortPreparation,
        },
        play_policy::{ClassifiedWindow, GaugeSelection, ResolvedPlayPolicy},
        replay_playback::decode_section_setup,
        PreparedBms,
    };
    use beatkernel::{
        audio::{AudioFormat, PcmLimits, SampleBank},
        input::DeviceId,
        judge::{JudgeGrade, JudgeWindow},
        time::{ClockDomainId, Duration, Timestamp},
    };
    use beatkernel_bms::{BmsGaugeKind, BmsJudgment};
    use std::{collections::BTreeMap, path::Path};

    fn prepared() -> PreparedBms {
        let source = beatkernel_bms::parse(
            "#BPM 60\n#TOTAL 320\n#WAV01 note.wav\n#00111:0101\n",
            Default::default(),
        )
        .unwrap();
        PreparedBms {
            compiled: source.compile().unwrap(),
            source,
            bank: SampleBank::new(
                AudioFormat::new(1000, 1).unwrap(),
                PcmLimits::new(64, 256, 4).unwrap(),
            )
            .unwrap(),
            sounds: vec![],
            bgm_commands: vec![],
        }
    }

    fn policy(
        prepared: &PreparedBms,
        kind: BmsGaugeKind,
        class: BmsJudgment,
    ) -> ResolvedPlayPolicy {
        ResolvedPlayPolicy::bms(
            &prepared.source,
            kind,
            &[ClassifiedWindow {
                judgment: class,
                window: JudgeWindow {
                    grade: JudgeGrade(7),
                    early: Duration::from_nanos(7),
                    late: Duration::from_nanos(9),
                },
            }],
            -3,
        )
        .unwrap()
    }

    fn config(bindings: &BTreeMap<u8, u16>, record: bool) -> CohortPreparation<'_> {
        CohortPreparation {
            host: ClockDomainId(99),
            output: ClockDomainId(2),
            early: 7,
            late: 9,
            offset: -3,
            preroll: 0,
            start: Timestamp::from_nanos(1),
            end: Some(Timestamp::from_nanos(5_000_000_000)),
            chart_seed: u64::MAX - 17,
            bindings,
            record_replay: record.then_some(Path::new("cohort-not-written.bkr")),
            replay_max_bytes: 65_536,
            replay_max_records: 128,
        }
    }

    #[test]
    fn genuine_selected_cohort_preparation_keeps_member_classes_gauges_section_seed_and_clock_axes()
    {
        let prepared = prepared();
        let bindings = BTreeMap::from([(0x11, 7)]);
        let assignments = [
            (PlayerId(u32::MAX), DeviceId(u64::MAX)),
            (PlayerId(7), DeviceId(31)),
        ];
        for kind in BmsGaugeKind::ALL {
            for class in [BmsJudgment::PGreat, BmsJudgment::Great] {
                let selected = policy(&prepared, kind, class);
                for record in [false, true] {
                    let cfg = config(&bindings, record);
                    for audio in [false, true] {
                        let cohort = if audio {
                            prepare_audio_cohort_with_policy(
                                &prepared,
                                &assignments,
                                &CompetitionOptions::default(),
                                &cfg,
                                ClockDomainId(17),
                                &selected,
                            )
                        } else {
                            prepare_cohort_with_policy(
                                &prepared,
                                &assignments,
                                &CompetitionOptions::default(),
                                &cfg,
                                &selected,
                            )
                        }
                        .unwrap();
                        assert!(cohort.network.is_none());
                        for (index, state) in cohort.states.iter().enumerate() {
                            assert_eq!(state.player, assignments[index].0);
                            assert_eq!(cohort.configs[index].player, assignments[index].0);
                            assert_eq!(cohort.configs[index].device, Some(assignments[index].1));
                            assert_eq!(cohort.configs[index].judge.profile(), selected.judge());
                            assert!(cohort.configs[index].judge.effective_song_time().is_none());
                            assert_eq!(state.gauge.profile(), selected.gauge());
                            assert_eq!(state.last_song, cfg.start);
                            assert_eq!(state.capture.is_some(), record);
                            if let Some(capture) = &state.capture {
                                let header = capture.header();
                                assert_eq!(
                                    header.normalized_clock,
                                    if audio { ClockDomainId(17) } else { cfg.host }
                                );
                                let setup = decode_section_setup(&header.options).unwrap();
                                assert_eq!(setup.judgments.as_ref(), selected.judgments());
                                assert_eq!(setup.gauge, *selected.gauge());
                                assert_eq!(setup.end, cfg.end);
                                assert_eq!(setup.start, cfg.start);
                                assert_eq!(setup.chart_seed, cfg.chart_seed);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn cold_policy_assignment_and_audio_axis_mismatches_precede_ghost_credentials_and_sockets() {
        let prepared = prepared();
        let selected = policy(&prepared, BmsGaugeKind::Hard, BmsJudgment::Great);
        let bindings = BTreeMap::from([(0x11, 7)]);
        let assignments = [
            (PlayerId(7), DeviceId(1)),
            (PlayerId(u32::MAX), DeviceId(2)),
        ];
        let mut competition = CompetitionOptions::default();
        competition.ghosts.push((
            OpponentKind::Other,
            "cohort-policy-must-not-open-missing-ghost.bkr".into(),
        ));
        competition.network = Some(NetworkRole::Host("127.0.0.1:34567".parse().unwrap()));
        competition.quic.cert = Some("cohort-policy-must-not-open-missing-cert.pem".into());
        competition.quic.key = Some("cohort-policy-must-not-open-missing-key.pem".into());
        let pristine: Vec<_> = prepared
            .compiled
            .chart
            .objects()
            .iter()
            .map(|object| (object.id, object.time.start, object.time.end))
            .collect();
        for record in [false, true] {
            let mut cfg = config(&bindings, record);
            cfg.early = 8;
            let failure = match prepare_cohort_with_policy(
                &prepared,
                &assignments,
                &competition,
                &cfg,
                &selected,
            ) {
                Err(error) => error,
                Ok(_) => panic!("mismatched selected windows must refuse before acquisition"),
            };
            assert_eq!(
                failure.to_string(),
                "cohort policy windows differ from native configuration"
            );
            let failure = match prepare_audio_cohort_with_policy(
                &prepared,
                &assignments,
                &competition,
                &cfg,
                ClockDomainId(17),
                &selected,
            ) {
                Err(error) => error,
                Ok(_) => panic!("mismatched audio policy must refuse before acquisition"),
            };
            assert_eq!(
                failure.to_string(),
                "audio cohort policy or clock configuration differs"
            );
            let cfg = config(&bindings, record);
            for invalid in [cfg.host, cfg.output] {
                let failure = match prepare_audio_cohort_with_policy(
                    &prepared,
                    &assignments,
                    &competition,
                    &cfg,
                    invalid,
                    &selected,
                ) {
                    Err(error) => error,
                    Ok(_) => panic!("aliased logical axis must refuse before acquisition"),
                };
                assert_eq!(
                    failure.to_string(),
                    "audio cohort policy or clock configuration differs"
                );
            }
            let duplicate = [(PlayerId(7), DeviceId(1)), (PlayerId(7), DeviceId(2))];
            let failure = match prepare_cohort_with_policy(
                &prepared,
                &duplicate,
                &competition,
                &cfg,
                &selected,
            ) {
                Err(error) => error,
                Ok(_) => panic!("duplicate original player must refuse before acquisition"),
            };
            assert_eq!(
                failure.to_string(),
                "cohort assignments require distinct positive player/device IDs"
            );
        }
        assert_eq!(
            prepared
                .compiled
                .chart
                .objects()
                .iter()
                .map(|object| (object.id, object.time.start, object.time.end))
                .collect::<Vec<_>>(),
            pristine
        );
        // Unchecked compatibility callers retain their conservative gate; this
        // integration enables only the genuine policy-aware preparation path.
        assert!(crate::native_judge::validate_policy_competition(
            GaugeSelection::Bms(BmsGaugeKind::Hard),
            &competition
        )
        .is_err());
    }
}
