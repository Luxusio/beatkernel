//! Native cold preparation preserves selected speed before saved/network acquisition.
use beatkernel::{
    audio::{AudioFormat, PcmLimits},
    input::DeviceId,
    replay::codec::{decode_replay, ReplayFile},
    time::{ClockDomainId, Duration, Timestamp},
};
use beatkernel_bms::BmsInputMode;
use beatkernel_bms_runtime::{
    competition::OpponentKind,
    competition_live::{replay_limits, CompetitionOptions, LiveCompetition, NetworkRole},
    input_sounds::InputSoundIdentity,
    live_speed::LiveSpeedSetup,
    local_players::PlayerId,
    native_cohort_setup::{prepare_audio_cohort_with_policy_and_speed, CohortPreparation},
    native_group_competition::NativeGroupCompetition,
    native_judge::{
        prepare_policy_competition_identity, prepare_policy_competition_identity_with_speed,
        prepare_policy_header, prepare_policy_header_with_speed,
        prepare_section_capture_for_policy, prepare_section_capture_for_policy_with_speed,
        NativeJudgeConfig,
    },
    play_policy::{GaugeSelection, OriginalGaugeContext, ResolvedPlayPolicy},
    replay_capture::LiveReplayCapture,
    replay_playback::{decode_section_setup, reconstruct_section, validate_section_setup},
    replay_speed::ReplaySpeed,
    ChannelPolicy, PreparedBms,
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn speed(value: &str) -> ReplaySpeed {
    value.parse().unwrap()
}
const PREROLL: i64 = 100_000_001;
fn judge_config() -> NativeJudgeConfig {
    NativeJudgeConfig {
        early: 10_000_000,
        late: 20_000_000,
        offset: -3,
        preroll: PREROLL,
        output: ClockDomainId(9),
        end: Some(ts(3_000_000_000)),
    }
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "beatkernel-live-preparation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(
            path.join("chart.bms"),
            b"#BPM 120\n#WAV01 key.wav\n#00011:01\n#00111:01\n",
        )
        .unwrap();
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&38u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&24_000u32.to_le_bytes());
        wav.extend_from_slice(&48_000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&2u32.to_le_bytes());
        wav.extend_from_slice(&8192i16.to_le_bytes());
        fs::write(path.join("key.wav"), wav).unwrap();
        Self(path)
    }
    fn prepared(&self) -> PreparedBms {
        beatkernel_bms_runtime::load_prepared_with_seed(
            &self.0.join("chart.bms"),
            AudioFormat::new(24_000, 1).unwrap(),
            PcmLimits::new(1024, 4096, 8).unwrap(),
            ChannelPolicy::Exact,
            0,
        )
        .unwrap()
    }
    fn policy(&self, prepared: &PreparedBms, bms: bool) -> ResolvedPlayPolicy {
        judge_config()
            .resolve_play_policy(
                &OriginalGaugeContext::from_source(&prepared.source),
                if bms {
                    GaugeSelection::Bms(beatkernel_bms::BmsGaugeKind::Hard)
                } else {
                    GaugeSelection::BeatKernel
                },
            )
            .unwrap()
    }
    fn saved(
        &self,
        prepared: &PreparedBms,
        policy: &ResolvedPlayPolicy,
        selected: ReplaySpeed,
    ) -> PathBuf {
        let judge = judge_config()
            .judge_with_policy(&prepared.source, prepared.compiled.chart.clone(), policy)
            .unwrap();
        // NORMAL builtin comparisons retain their historical endpoint-free
        // recording bytes. Selected-speed records come from the real public
        // capture API, with the complete selected policy and section identity.
        let capture =
            if policy.selection() == GaugeSelection::BeatKernel && selected == speed("1/1") {
                let legacy = LiveReplayCapture::new_with_input_sounds(
                    &judge,
                    ClockDomainId(71),
                    replay_limits().unwrap(),
                    Timestamp::ZERO,
                    0,
                    None,
                    BmsInputMode::ButtonOnly,
                    InputSoundIdentity::from_source(&prepared.source).unwrap(),
                )
                .unwrap();
                legacy
            } else {
                prepare_section_capture_for_policy_with_speed(
                    &prepared.source,
                    &judge,
                    policy,
                    ClockDomainId(71),
                    Timestamp::ZERO,
                    0,
                    judge_config().end,
                    Some(replay_limits().unwrap()),
                    selected,
                )
                .unwrap()
                .unwrap()
            };
        let bytes = capture.into_bytes().unwrap();
        let file = decode_replay(&bytes, replay_limits().unwrap()).unwrap();
        assert_eq!(
            decode_section_setup(&file.header.options)
                .unwrap()
                .source_speed,
            selected
        );
        if selected != speed("1/1") {
            assert_eq!(
                decode_section_setup(&file.header.options).unwrap().end,
                judge_config().end
            );
        }
        // Genuine parsers/reconstruction prove this is a recording, not an
        // arbitrary matching byte token assembled to satisfy admission.
        reconstruct_section(&prepared.source, file.clone(), replay_limits().unwrap()).unwrap();
        let path = self.0.join("saved.bkr");
        fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn native_headers_and_capture_reconstruct_actual_selected_source_speed() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    for bms in [false, true] {
        let policy = fixture.policy(&prepared, bms);
        let judge = judge_config()
            .judge_with_policy(&prepared.source, prepared.compiled.chart.clone(), &policy)
            .unwrap();
        for value in ["1/2", "1/1", "3/2", "2/1"] {
            let selected = speed(value);
            let header = prepare_policy_header_with_speed(
                &prepared.source,
                &judge,
                &policy,
                ClockDomainId(7),
                Timestamp::ZERO,
                0,
                judge_config().end,
                selected,
            )
            .unwrap();
            let capture = prepare_section_capture_for_policy_with_speed(
                &prepared.source,
                &judge,
                &policy,
                ClockDomainId(7),
                Timestamp::ZERO,
                0,
                judge_config().end,
                Some(replay_limits().unwrap()),
                selected,
            )
            .unwrap()
            .unwrap();
            assert_eq!(capture.header(), &header);
            let setup = decode_section_setup(&header.options).unwrap();
            assert_eq!(setup.source_speed, selected);
            assert_eq!(setup.end, judge_config().end);
            assert_eq!(setup.start, Timestamp::ZERO);
            assert_eq!(setup.profile, *policy.judge());
            let file = capture.into_file();
            validate_section_setup(&prepared.source, &file, replay_limits().unwrap()).unwrap();
            let replay =
                reconstruct_section(&prepared.source, file, replay_limits().unwrap()).unwrap();
            assert_eq!(replay.engine().profile(), policy.judge());
        }
    }
}

#[test]
fn normal_native_header_capture_and_network_identity_preserve_legacy_bytes() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    for bms in [false, true] {
        let policy = fixture.policy(&prepared, bms);
        let judge = judge_config()
            .judge_with_policy(&prepared.source, prepared.compiled.chart.clone(), &policy)
            .unwrap();
        assert_eq!(
            prepare_policy_header(
                &prepared.source,
                &judge,
                &policy,
                ClockDomainId(7),
                Timestamp::ZERO,
                0,
                judge_config().end
            )
            .unwrap(),
            prepare_policy_header_with_speed(
                &prepared.source,
                &judge,
                &policy,
                ClockDomainId(7),
                Timestamp::ZERO,
                0,
                judge_config().end,
                speed("1/1")
            )
            .unwrap()
        );
        let legacy = prepare_section_capture_for_policy(
            &prepared.source,
            &judge,
            &policy,
            ClockDomainId(7),
            Timestamp::ZERO,
            0,
            judge_config().end,
            Some(replay_limits().unwrap()),
        )
        .unwrap()
        .unwrap()
        .into_bytes()
        .unwrap();
        let selected = prepare_section_capture_for_policy_with_speed(
            &prepared.source,
            &judge,
            &policy,
            ClockDomainId(7),
            Timestamp::ZERO,
            0,
            judge_config().end,
            Some(replay_limits().unwrap()),
            speed("1/1"),
        )
        .unwrap()
        .unwrap()
        .into_bytes()
        .unwrap();
        assert_eq!(legacy, selected);
        assert_eq!(
            prepare_policy_competition_identity(
                &prepared.source,
                &judge,
                &policy,
                ClockDomainId(7),
                Timestamp::ZERO,
                0,
                judge_config().end
            )
            .unwrap(),
            prepare_policy_competition_identity_with_speed(
                &prepared.source,
                &judge,
                &policy,
                ClockDomainId(7),
                Timestamp::ZERO,
                0,
                judge_config().end,
                speed("1/1")
            )
            .unwrap()
        );
    }
}

#[test]
fn network_identity_normalizes_clock_and_fraction_but_retains_source_speed() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    let policy = fixture.policy(&prepared, false);
    let judge = judge_config()
        .judge_with_policy(&prepared.source, prepared.compiled.chart.clone(), &policy)
        .unwrap();
    let identity = |domain, selected| {
        prepare_policy_competition_identity_with_speed(
            &prepared.source,
            &judge,
            &policy,
            ClockDomainId(domain),
            Timestamp::ZERO,
            0,
            None,
            selected,
        )
        .unwrap()
    };
    let a = identity(7, speed("6/4"));
    assert_eq!(a, identity(81, speed("3/2")));
    assert_ne!(a, identity(7, speed("1/1")));
    assert_ne!(a, identity(7, speed("2/1")));
    let decoded = decode_replay(&a, replay_limits().unwrap()).unwrap();
    assert_eq!(decoded.header.normalized_clock, ClockDomainId(0));
    assert_eq!(
        decode_section_setup(&decoded.header.options)
            .unwrap()
            .source_speed,
        speed("3/2")
    );
    validate_section_setup(&prepared.source, &decoded, replay_limits().unwrap()).unwrap();
}

#[test]
fn saved_own_and_other_records_require_same_source_speed_for_solo_and_member() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    for bms in [false, true] {
        let policy = fixture.policy(&prepared, bms);
        let judge = judge_config()
            .judge_with_policy(&prepared.source, prepared.compiled.chart.clone(), &policy)
            .unwrap();
        for original in ["1/1", "3/2"] {
            let saved = fixture.saved(&prepared, &policy, speed(original));
            for kind in [OpponentKind::Own, OpponentKind::Other] {
                let options = CompetitionOptions {
                    ghosts: vec![(kind, saved.clone())],
                    ..CompetitionOptions::default()
                };
                for member in [false, true] {
                    let prepare = |selected, end| {
                        if member {
                            LiveCompetition::prepare_member_section_with_policy_and_speed(
                                PlayerId(43),
                                &options,
                                &prepared.source,
                                &judge,
                                &policy,
                                ClockDomainId(7),
                                Timestamp::ZERO,
                                0,
                                end,
                                selected,
                            )
                        } else {
                            LiveCompetition::prepare_native_section_with_policy_and_speed(
                                &options,
                                &prepared.source,
                                &judge,
                                &policy,
                                ClockDomainId(7),
                                Timestamp::ZERO,
                                0,
                                end,
                                PREROLL,
                                selected,
                            )
                        }
                    };
                    let owner = prepare(speed(original), judge_config().end)
                        .unwrap()
                        .unwrap();
                    let snapshot = owner.archive_snapshot().unwrap();
                    assert_eq!(snapshot.ghosts.len(), 1);
                    assert_eq!(snapshot.ghosts[0].kind, kind);
                    assert!(snapshot.network.is_none());
                    assert!(
                        prepare(speed("2/1"), judge_config().end).is_err(),
                        "different source speed admitted"
                    );
                    if original != "1/1" {
                        assert!(
                            prepare(speed(original), Some(ts(3_000_000_001))).is_err(),
                            "different recorded section admitted"
                        );
                    }
                }
            }
        }
    }
}

fn config<'a>(
    bindings: &'a BTreeMap<u8, u16>,
    record: &'a std::path::Path,
) -> CohortPreparation<'a> {
    CohortPreparation {
        host: ClockDomainId(1),
        output: ClockDomainId(2),
        early: 10_000_000,
        late: 20_000_000,
        offset: -3,
        preroll: PREROLL,
        start: Timestamp::ZERO,
        end: judge_config().end,
        chart_seed: 0,
        bindings,
        record_replay: Some(record),
        replay_max_bytes: 1 << 20,
        replay_max_records: 100,
    }
}

#[test]
fn two_and_four_member_captures_keep_exact_preroll_phase_and_matching_saved_record() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    let bindings = BTreeMap::from([(0x11, 4)]);
    let record = fixture.0.join("capture.bkr");
    let config = config(&bindings, &record);
    for bms in [false, true] {
        let policy = fixture.policy(&prepared, bms);
        for rate in ["1/2", "1/1", "3/2", "2/1"] {
            let selected = speed(rate);
            let saved = fixture.saved(&prepared, &policy, selected);
            let competition = CompetitionOptions {
                ghosts: vec![(OpponentKind::Own, saved)],
                ..CompetitionOptions::default()
            };
            let setup = LiveSpeedSetup::new(
                config.start,
                Duration::from_nanos(PREROLL),
                selected,
                config.end,
            )
            .unwrap();
            for count in [2u32, 4] {
                let assignments: Vec<_> = (1..=count)
                    .map(|id| (PlayerId(id), DeviceId(u64::from(id) + 10)))
                    .collect();
                let cohort = prepare_audio_cohort_with_policy_and_speed(
                    &prepared,
                    &assignments,
                    &competition,
                    &config,
                    ClockDomainId(3),
                    &policy,
                    setup,
                )
                .unwrap();
                assert_eq!(cohort.configs.len(), count as usize);
                assert_eq!(cohort.states.len(), count as usize);
                assert!(cohort.network.is_none());
                for (member, state) in cohort.configs.iter().zip(&cohort.states) {
                    assert_eq!(state.player, member.player);
                    // Independent wide signed truncation, including odd preroll.
                    let expected = -i128::from(PREROLL) * i128::from(selected.rate().numerator())
                        / i128::from(selected.rate().denominator());
                    assert_eq!(state.last_song, ts(i64::try_from(expected).unwrap()));
                    let capture = state.capture.as_ref().unwrap();
                    assert_eq!(capture.header().normalized_clock, ClockDomainId(3));
                    let decoded = decode_section_setup(&capture.header().options).unwrap();
                    assert_eq!(decoded.source_speed, selected);
                    assert_eq!(decoded.end, config.end);
                    assert_eq!(decoded.profile, *member.judge.profile());
                    validate_section_setup(
                        &prepared.source,
                        &ReplayFile::new(capture.header().clone(), vec![]),
                        replay_limits().unwrap(),
                    )
                    .unwrap();
                    assert_eq!(
                        state
                            .competition
                            .as_ref()
                            .unwrap()
                            .archive_snapshot()
                            .unwrap()
                            .ghosts
                            .len(),
                        1
                    );
                }
                for (_, path) in cohort.save_paths {
                    assert!(!path.unwrap().exists());
                }
            }
        }
    }
}

#[test]
fn cohort_cold_mismatch_is_rejected_before_missing_saved_file_or_network_credentials() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    let policy = fixture.policy(&prepared, false);
    let bindings = BTreeMap::from([(0x11, 4)]);
    let record = fixture.0.join("never-written.bkr");
    let config = config(&bindings, &record);
    let assignments = [(PlayerId(1), DeviceId(11)), (PlayerId(2), DeviceId(12))];
    let options = CompetitionOptions {
        ghosts: vec![(OpponentKind::Own, fixture.0.join("missing.bkr"))],
        network: Some(NetworkRole::Join("127.0.0.1:1".parse().unwrap())),
        ..CompetitionOptions::default()
    };
    for (start, preroll, end) in [
        (ts(1), PREROLL, config.end),
        (config.start, PREROLL + 1, config.end),
        (config.start, PREROLL, Some(ts(3_000_000_001))),
    ] {
        let setup =
            LiveSpeedSetup::new(start, Duration::from_nanos(preroll), speed("3/2"), end).unwrap();
        let error = prepare_audio_cohort_with_policy_and_speed(
            &prepared,
            &assignments,
            &options,
            &config,
            ClockDomainId(3),
            &policy,
            setup,
        )
        .err()
        .unwrap()
        .to_string();
        assert!(
            error.contains("configuration") || error.contains("setup") || error.contains("differs"),
            "must refuse the actual cold mismatch before acquisition: {error}"
        );
        assert!(
            !error.contains("No such file") && !error.contains("credential"),
            "{error}"
        );
        assert!(!record.exists());
    }
}

#[test]
fn nonpristine_judge_refuses_before_saved_file_or_network_acquisition() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    let policy = fixture.policy(&prepared, false);
    let mut judge = judge_config()
        .judge_with_policy(&prepared.source, prepared.compiled.chart.clone(), &policy)
        .unwrap();
    judge.advance_to(ts(-1)).unwrap();
    let options = CompetitionOptions {
        ghosts: vec![(OpponentKind::Own, fixture.0.join("missing.bkr"))],
        network: Some(NetworkRole::Join("127.0.0.1:1".parse().unwrap())),
        ..CompetitionOptions::default()
    };
    let error = LiveCompetition::prepare_native_section_with_policy_and_speed(
        &options,
        &prepared.source,
        &judge,
        &policy,
        ClockDomainId(7),
        Timestamp::ZERO,
        0,
        judge_config().end,
        PREROLL,
        speed("3/2"),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("pristine"), "{error}");
}

#[test]
fn disabled_selected_capture_preserves_existing_none_behavior() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    let policy = fixture.policy(&prepared, false);
    let judge = judge_config()
        .judge_with_policy(&prepared.source, prepared.compiled.chart.clone(), &policy)
        .unwrap();
    assert!(prepare_section_capture_for_policy_with_speed(
        &prepared.source,
        &judge,
        &policy,
        ClockDomainId(7),
        Timestamp::ZERO,
        0,
        judge_config().end,
        None,
        speed("3/2")
    )
    .unwrap()
    .is_none());
}

#[test]
fn group_validates_every_original_member_before_network_acquisition() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    let policy = fixture.policy(&prepared, false);
    let bindings = BTreeMap::from([(0x11, 4)]);
    let record = fixture.0.join("capture.bkr");
    let config = config(&bindings, &record);
    let assignments = [(PlayerId(1), DeviceId(11)), (PlayerId(2), DeviceId(12))];
    let setup = LiveSpeedSetup::new(
        config.start,
        Duration::from_nanos(PREROLL),
        speed("3/2"),
        config.end,
    )
    .unwrap();
    let mut cohort = prepare_audio_cohort_with_policy_and_speed(
        &prepared,
        &assignments,
        &CompetitionOptions::default(),
        &config,
        ClockDomainId(3),
        &policy,
        setup,
    )
    .unwrap();
    let policies = [(PlayerId(1), &policy), (PlayerId(2), &policy)];
    assert!(NativeGroupCompetition::prepare_with_policies_and_speed(
        &CompetitionOptions::default(),
        &prepared.source,
        &cohort.configs,
        &policies,
        ClockDomainId(3),
        config.start,
        0,
        config.end,
        PREROLL,
        speed("3/2")
    )
    .unwrap()
    .is_none());
    let options = CompetitionOptions {
        network: Some(NetworkRole::Join("127.0.0.1:1".parse().unwrap())),
        ..CompetitionOptions::default()
    };
    let reordered = [(PlayerId(2), &policy), (PlayerId(1), &policy)];
    let error = NativeGroupCompetition::prepare_with_policies_and_speed(
        &options,
        &prepared.source,
        &cohort.configs,
        &reordered,
        ClockDomainId(3),
        config.start,
        0,
        config.end,
        PREROLL,
        speed("3/2"),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("roster"), "{error}");
    cohort.configs[1].judge.advance_to(ts(-1)).unwrap();
    let error = NativeGroupCompetition::prepare_with_policies_and_speed(
        &options,
        &prepared.source,
        &cohort.configs,
        &policies,
        ClockDomainId(3),
        config.start,
        0,
        config.end,
        PREROLL,
        speed("3/2"),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(
        error.contains("pristine"),
        "last member must be checked before network acquisition: {error}"
    );
}
