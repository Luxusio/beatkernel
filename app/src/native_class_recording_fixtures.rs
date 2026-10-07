//! Integration contracts for selected class identity, through real native preparation.
use crate::{
    competition::{OpponentKind, ScoreSummary},
    competition_live::{CompetitionOptions, LiveCompetition, replay_limits},
    judgment_policy::{BmsJudgmentPolicy, GradeClass},
    native_gameplay::NativeGameplayConfig,
    native_judge::{NativeJudgeConfig, prepare_section_capture_for_policy},
    play_policy::{GaugeSelection, OriginalGaugeContext, ResolvedPlayPolicy},
    record_catalog::RecordPreview,
    replay_capture::LiveReplayCapture,
    replay_playback::{decode_section_setup, reconstruct_section},
    settings::{NativeSettings, SettingsHost},
};
use beatkernel::{
    audio::command_queue,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::JudgeEngine,
    replay::codec::{ReplayFile, decode_replay, encode_replay},
    runtime::{Runtime, RuntimeProcessingClock},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsGaugeKind, BmsJudgment};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const CHART: &str = "#BPM 60\n#TOTAL 300\n#WAV01 note.wav\n#00011:01\n";
fn source() -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(CHART, Default::default()).unwrap()
}
fn config() -> NativeJudgeConfig {
    NativeJudgeConfig {
        early: 0,
        late: 0,
        offset: 0,
        preroll: 0,
        output: ClockDomainId(17),
        end: None,
    }
}
fn policy(source: &beatkernel_bms::BmsChart, kind: BmsGaugeKind) -> ResolvedPlayPolicy {
    config()
        .resolve_play_policy(
            &OriginalGaugeContext::from_source(source),
            GaugeSelection::Bms(kind),
        )
        .unwrap()
}
fn judge(source: &beatkernel_bms::BmsChart, policy: &ResolvedPlayPolicy) -> JudgeEngine {
    config()
        .judge_with_policy(source, source.compile().unwrap().chart, policy)
        .unwrap()
}
fn capture(
    source: &beatkernel_bms::BmsChart,
    judge: &JudgeEngine,
    policy: &ResolvedPlayPolicy,
    end: Option<Timestamp>,
) -> LiveReplayCapture {
    prepare_section_capture_for_policy(
        source,
        judge,
        policy,
        ClockDomainId(17),
        Timestamp::ZERO,
        0,
        end,
        Some(replay_limits().unwrap()),
    )
    .unwrap()
    .unwrap()
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        (point.domain == target).then_some(point.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(17),
        timestamp: Timestamp::from_nanos(ns),
    }
}
/// A saved prefix comes from committed Runtime reports, including a real hit.
fn recorded(
    source: &beatkernel_bms::BmsChart,
    policy: &ResolvedPlayPolicy,
    end: Option<Timestamp>,
) -> ReplayFile {
    let engine = judge(source, policy);
    let mut capture = capture(source, &engine, policy, end);
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(7u16),
        game_control: GameControlId(0x11),
    }])
    .unwrap();
    let (producer, _consumer) = command_queue(8).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(17),
        ClockDomainId(17),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings,
        engine,
        producer,
        vec![],
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let report = runtime
        .process_input(
            PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(DeviceId(9), point(0), 0),
                control: PhysicalControlId::keyboard(7u16),
                state: ButtonState::Down,
            }),
            &Identity,
            point(0),
        )
        .unwrap();
    let mut score = ScoreSummary::default();
    score.observe(&report.judge_events).unwrap();
    assert_eq!(score.hits, 1);
    assert_eq!(score.misses, 0);
    capture.record_report(&report).unwrap();
    let report = runtime.advance_to(point(1), &Identity, point(1)).unwrap();
    capture.record_report(&report).unwrap();
    capture.into_file()
}
struct Saved(PathBuf);
impl Saved {
    fn bytes(bytes: &[u8], extension: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "beatkernel-native-classes-{}-{}.{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            extension
        ));
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }
    fn replay(file: &ReplayFile) -> Self {
        Self::bytes(
            &encode_replay(file, replay_limits().unwrap()).unwrap(),
            "bkr",
        )
    }
}
impl Drop for Saved {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn reclassify(file: &ReplayFile, class: BmsJudgment) -> ReplayFile {
    let setup = decode_section_setup(&file.header.options).unwrap();
    let classes = BmsJudgmentPolicy::new(
        &setup
            .profile
            .windows()
            .iter()
            .map(|window| GradeClass {
                grade: window.grade,
                class,
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let (inner, _) = crate::replay_judgment_policy::split_options(&file.header.options).unwrap();
    let mut changed = file.clone();
    changed.header.options = inner.to_vec();
    changed.header = crate::replay_judgment_policy::wrap_header(
        changed.header,
        Some(&classes),
        replay_limits().unwrap(),
    )
    .unwrap();
    changed
}
fn legacy(file: &ReplayFile) -> ReplayFile {
    let mut legacy = file.clone();
    legacy.header.options = crate::replay_judgment_policy::split_options(&file.header.options)
        .unwrap()
        .0
        .to_vec();
    legacy
}
fn gameplay_config(end: Option<Timestamp>) -> NativeGameplayConfig {
    NativeGameplayConfig {
        origin: point(0),
        stream_origin: point(0),
        playback_origin: point(0),
        song_origin: Timestamp::ZERO,
        sample_rate: 1000,
        end_song: end,
        advance_lag: Duration::from_nanos(10_000_000),
        seconds: None,
        pause_supported: false,
        logical_schedule: true,
    }
}

#[test]
fn selected_common_capture_records_exact_classes_and_actual_runtime_for_all_six_gauges() {
    let source = source();
    for kind in BmsGaugeKind::ALL {
        let policy = policy(&source, kind);
        for end in [None, Some(Timestamp::from_nanos(10))] {
            let file = recorded(&source, &policy, end);
            let limits = replay_limits().unwrap();
            let decoded = decode_replay(&encode_replay(&file, limits).unwrap(), limits).unwrap();
            assert_eq!(decoded, file);
            let setup = decode_section_setup(&decoded.header.options).unwrap();
            assert_eq!(setup.judgments.as_ref(), policy.judgments());
            let classes = setup
                .judgments
                .as_ref()
                .expect("selected capture is classified");
            classes.validate_profile(&setup.profile).unwrap();
            assert_eq!(classes.entries().len(), setup.profile.windows().len());
            for window in setup.profile.windows() {
                assert_eq!(classes.class(window.grade), Some(BmsJudgment::PGreat));
            }
            assert_eq!(setup.profile, *policy.judge());
            assert_eq!(setup.gauge, *policy.gauge());
            assert_eq!(setup.end, end);
            let replay = reconstruct_section(&source, decoded, limits).unwrap();
            let mut score = ScoreSummary::default();
            score.observe(replay.results()).unwrap();
            assert_eq!(score.hits, 1);
            assert_eq!(classes.project(&score).unwrap().ex_score, 2);
        }
    }
}

#[test]
fn cold_native_admission_checks_class_coverage_and_canonical_metadata_for_all_six_gauges() {
    let source = source();
    for kind in BmsGaugeKind::ALL {
        let policy = policy(&source, kind);
        let engine = judge(&source, &policy);
        let capture = capture(&source, &engine, &policy, None);
        let header = capture.header();
        crate::native_policy_admission::validate_header(
            &engine,
            policy.gauge(),
            header,
            &gameplay_config(None),
        )
        .unwrap();
        // The last row is one u32 grade and one bounded class tag.
        for mutation in 0..4 {
            let mut malformed = header.clone();
            match mutation {
                0 => {
                    malformed.options.push(0);
                }
                1 => {
                    *malformed.options.last_mut().unwrap() = 4;
                }
                2 => {
                    malformed.options.pop();
                }
                _ => {
                    let grade_start = malformed.options.len() - 5;
                    let unknown = engine.profile().windows()[0].grade.0.wrapping_add(1);
                    malformed.options[grade_start..grade_start + 4]
                        .copy_from_slice(&unknown.to_le_bytes());
                }
            }
            assert!(
                crate::native_policy_admission::validate_header(
                    &engine,
                    policy.gauge(),
                    &malformed,
                    &gameplay_config(None)
                )
                .is_err(),
                "mutation {mutation}"
            );
            assert!(engine.effective_song_time().is_none());
            assert!(capture.records().is_empty());
        }
    }
}

#[test]
fn builtin_bytes_and_legacy_gauge_helpers_remain_unchanged_without_guessed_classes() {
    let source = source();
    let builtin = ResolvedPlayPolicy::builtin(0, 0, 0).unwrap();
    for policy in std::iter::once(builtin).chain(
        BmsGaugeKind::ALL
            .into_iter()
            .map(|kind| policy(&source, kind)),
    ) {
        let engine = judge(&source, &policy);
        let selected = capture(&source, &engine, &policy, None).into_file();
        let old = LiveReplayCapture::new_with_gauge(
            &engine,
            ClockDomainId(17),
            replay_limits().unwrap(),
            Timestamp::ZERO,
            0,
            None,
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            policy.gauge(),
        )
        .unwrap()
        .into_file();
        assert!(
            decode_section_setup(&old.header.options)
                .unwrap()
                .judgments
                .is_none()
        );
        assert_eq!(legacy(&selected), old);
        if policy.selection() == GaugeSelection::BeatKernel {
            assert_eq!(
                encode_replay(&selected, replay_limits().unwrap()).unwrap(),
                encode_replay(&old, replay_limits().unwrap()).unwrap()
            );
        } else {
            assert_ne!(selected.header.options, old.header.options);
        }
    }
}

#[test]
fn disabled_capture_checks_pristine_matching_profile_without_source_identity_acquisition() {
    let mut source = beatkernel_bms::parse(
        "#BPM 60\n#WAV01 note.wav\n#00011:01\n#00031:01\n",
        Default::default(),
    )
    .unwrap();
    // This invalid source-side sound identity is only touched by enabled capture.
    source.metadata.insert("VOLWAV".into(), "bad".into());
    for kind in BmsGaugeKind::ALL {
        let policy = policy(&source, kind);
        let mut engine = judge(&source, &policy);
        assert!(
            prepare_section_capture_for_policy(
                &source,
                &engine,
                &policy,
                ClockDomainId(17),
                Timestamp::ZERO,
                0,
                None,
                None
            )
            .unwrap()
            .is_none()
        );
        assert!(
            prepare_section_capture_for_policy(
                &source,
                &engine,
                &policy,
                ClockDomainId(17),
                Timestamp::ZERO,
                0,
                None,
                Some(replay_limits().unwrap())
            )
            .is_err()
        );
        let wrong = ResolvedPlayPolicy::builtin(1, 0, 0).unwrap();
        assert!(
            prepare_section_capture_for_policy(
                &source,
                &engine,
                &wrong,
                ClockDomainId(17),
                Timestamp::ZERO,
                0,
                None,
                None
            )
            .is_err()
        );
        assert!(engine.effective_song_time().is_none());
        engine.advance_to(Timestamp::ZERO).unwrap();
        assert!(
            prepare_section_capture_for_policy(
                &source,
                &engine,
                &policy,
                ClockDomainId(17),
                Timestamp::ZERO,
                0,
                None,
                None
            )
            .is_err()
        );
    }
}

#[test]
fn saved_own_and_other_admission_keeps_classes_and_refuses_equal_gauge_different_classes() {
    let source = source();
    for kind in BmsGaugeKind::ALL {
        let policy = policy(&source, kind);
        for end in [None, Some(Timestamp::from_nanos(10))] {
            let file = recorded(&source, &policy, end);
            let saved = Saved::replay(&file);
            let different = reclassify(&file, BmsJudgment::Great);
            let old = legacy(&file);
            assert_eq!(
                decode_section_setup(&different.header.options)
                    .unwrap()
                    .gauge,
                *policy.gauge()
            );
            assert_eq!(
                decode_section_setup(&different.header.options)
                    .unwrap()
                    .profile,
                *policy.judge()
            );
            assert!(
                decode_section_setup(&old.header.options)
                    .unwrap()
                    .judgments
                    .is_none()
            );
            reconstruct_section(&source, old.clone(), replay_limits().unwrap()).unwrap();
            for kind in [OpponentKind::Own, OpponentKind::Other] {
                let mut options = CompetitionOptions::default();
                options.ghosts.push((kind, saved.0.clone()));
                let engine = judge(&source, &policy);
                let live = LiveCompetition::prepare_native_section_with_policy(
                    &options,
                    &source,
                    &engine,
                    &policy,
                    ClockDomainId(17),
                    Timestamp::ZERO,
                    0,
                    end,
                    0,
                )
                .unwrap()
                .unwrap();
                assert_eq!(live.native_policy_header(), &file.header);
                assert_eq!(live.archive_snapshot().unwrap().ghosts.len(), 1);
                for refused in [&different, &old] {
                    let refused = Saved::replay(refused);
                    let mut wrong = CompetitionOptions::default();
                    wrong.ghosts.push((kind, refused.0.clone()));
                    assert!(
                        LiveCompetition::prepare_native_section_with_policy(
                            &wrong,
                            &source,
                            &engine,
                            &policy,
                            ClockDomainId(17),
                            Timestamp::ZERO,
                            0,
                            end,
                            0
                        )
                        .is_err()
                    );
                    assert!(engine.effective_song_time().is_none());
                }
            }
        }
    }
}

#[test]
fn selected_shared_cohort_preserves_class_headers_original_ids_and_endpoints() {
    use crate::{
        PreparedBms,
        local_players::PlayerId,
        native_cohort_setup::{CohortPreparation, prepare_cohort_with_policy},
    };
    use beatkernel::audio::{AudioFormat, PcmLimits, SampleBank};
    let source = source();
    let prepared = PreparedBms {
        compiled: source.compile().unwrap(),
        source,
        bank: SampleBank::new(
            AudioFormat::new(1000, 1).unwrap(),
            PcmLimits::new(64, 256, 4).unwrap(),
        )
        .unwrap(),
        sounds: vec![],
        bgm_commands: vec![],
    };
    let bindings = std::collections::BTreeMap::from([(0x11, 7)]);
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    for kind in BmsGaugeKind::ALL {
        let policy = policy(&prepared.source, kind);
        for end in [None, Some(Timestamp::from_nanos(10))] {
            let file = recorded(&prepared.source, &policy, end);
            let saved = Saved::replay(&file);
            let mut options = CompetitionOptions::default();
            options.ghosts.push((OpponentKind::Other, saved.0.clone()));
            let config = CohortPreparation {
                host: ClockDomainId(17),
                output: ClockDomainId(2),
                early: 0,
                late: 0,
                offset: 0,
                preroll: 0,
                start: Timestamp::ZERO,
                end,
                chart_seed: 0,
                bindings: &bindings,
                record_replay: Some(std::path::Path::new("not-written.bkr")),
                replay_max_bytes: 65536,
                replay_max_records: 128,
            };
            let cohort = prepare_cohort_with_policy(
                &prepared,
                &[(ids[0], DeviceId(1)), (ids[1], DeviceId(2))],
                &options,
                &config,
                &policy,
            )
            .unwrap();
            assert_eq!(cohort.states.len(), ids.len());
            for (index, state) in cohort.states.iter().enumerate() {
                assert_eq!(state.player, ids[index]);
                assert_eq!(cohort.configs[index].player, ids[index]);
                assert_eq!(cohort.save_paths[index].0, ids[index]);
                let capture = state.capture.as_ref().unwrap();
                assert_eq!(capture.header(), &file.header);
                assert_eq!(
                    state.competition.as_ref().unwrap().native_policy_header(),
                    capture.header()
                );
                let setup = decode_section_setup(&capture.header().options).unwrap();
                assert_eq!(setup.judgments.as_ref(), policy.judgments());
                assert_eq!(setup.end, end);
            }
        }
    }
}

#[test]
fn public_preview_accepts_selected_saved_classes_and_refuses_legacy_or_changed_class() {
    let source = source();
    let chart = Saved::bytes(CHART.as_bytes(), "bms");
    let names = ["assist-easy", "easy", "groove", "hard", "ex-hard", "hazard"];
    for (kind, name) in BmsGaugeKind::ALL.into_iter().zip(names) {
        let policy = policy(&source, kind);
        for end in [None, Some(Timestamp::from_nanos(10))] {
            let mut args = vec![
                "--gauge".into(),
                name.into(),
                "--early-ns".into(),
                "0".into(),
                "--late-ns".into(),
                "0".into(),
            ];
            if let Some(end) = end {
                args.extend(["--end-ns".into(), end.as_nanos().to_string()]);
            }
            let settings = NativeSettings::from_args(&args, SettingsHost::Linux).unwrap();
            let file = recorded(&source, &policy, end);
            let saved = Saved::replay(&file);
            let preview = RecordPreview::inspect(&saved.0, &chart.0, &settings).unwrap();
            assert_eq!(preview.score.hits, 1);
            assert_eq!(preview.score.misses, 0);
            assert_eq!(preview.recorded_until, Some(Timestamp::from_nanos(1)));
            assert_eq!(preview.end, end);
            for refused in [legacy(&file), reclassify(&file, BmsJudgment::Great)] {
                let saved = Saved::replay(&refused);
                assert!(RecordPreview::inspect(&saved.0, &chart.0, &settings).is_err());
            }
        }
    }
}
