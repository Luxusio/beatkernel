//! Class scores cross genuine Runtime capture, public preview, and retained geometry.
use crate::{
    competition::ScoreSummary,
    competition_live::replay_limits,
    gauge::{BmsGauge, GaugeProfile},
    historical_record_presentation::HistoricalRecordPresentation,
    judgment_policy::BmsScoreSummary,
    local_players::PlayerId,
    native_judge::{NativeJudgeConfig, prepare_section_capture_for_policy},
    play_policy::{ClassifiedWindow, GaugeSelection, OriginalGaugeContext, ResolvedPlayPolicy},
    play_result::CompletedPlayResult,
    record_catalog::{RecordCatalog, RecordPreview},
    replay_capture::LiveReplayCapture,
    result_archive::{ArchivedScore, ResultArchive, decode_archive, encode_archive},
    scene::Scene,
    screen_lifecycle::ScreenInstanceId,
    settings::{NativeSettings, SettingsHost},
    ui::{
        interaction::{Bounds, ControlId},
        records::{RecordsFrame, RecordsView},
        text_input::LineEditor,
    },
};
use beatkernel::{
    audio::command_queue,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeGrade, JudgeWindow},
    replay::codec::{ReplayFile, decode_replay, encode_replay},
    runtime::{Runtime, RuntimeProcessingClock},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsGaugeKind, BmsInputMode, BmsJudgment};
use std::{path::Path, sync::Arc};

fn source() -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(
        "#BPM 60\n#TOTAL 320\n#WAV01 x.wav\n#00011:01010101\n#00111:01\n",
        Default::default(),
    )
    .unwrap()
}
fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(17),
        timestamp: Timestamp::from_nanos(ns),
    }
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
/// Native selected policy is one PGREAT window. The other lane supplies real
/// four-class reports to the pure historical model without bypassing draft admission.
fn actual_capture(
    selected: bool,
    finite: bool,
) -> (ReplayFile, ScoreSummary, CompletedPlayResult, GaugeProfile) {
    actual_capture_with_policy(selected, finite, false)
}
fn actual_capture_with_policy(
    selected: bool,
    finite: bool,
    builtin: bool,
) -> (ReplayFile, ScoreSummary, CompletedPlayResult, GaugeProfile) {
    let source = source();
    let config = NativeJudgeConfig {
        early: 4,
        late: 4,
        offset: 0,
        preroll: 0,
        output: ClockDomainId(17),
        end: None,
    };
    let policy = if builtin {
        ResolvedPlayPolicy::builtin(4, 4, 0).unwrap()
    } else if selected {
        config
            .resolve_play_policy(
                &OriginalGaugeContext::from_source(&source),
                GaugeSelection::Bms(BmsGaugeKind::Groove),
            )
            .unwrap()
    } else {
        let windows: Vec<_> = [
            (91, BmsJudgment::PGreat),
            (7, BmsJudgment::Great),
            (63, BmsJudgment::Good),
            (2, BmsJudgment::Bad),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (grade, judgment))| ClassifiedWindow {
            judgment,
            window: JudgeWindow {
                grade: JudgeGrade(grade),
                early: Duration::from_nanos(index as i64 + 1),
                late: Duration::from_nanos(index as i64 + 1),
            },
        })
        .collect();
        ResolvedPlayPolicy::bms(&source, BmsGaugeKind::Groove, &windows, 0).unwrap()
    };
    let judge = config
        .judge_with_policy(&source, source.compile().unwrap().chart, &policy)
        .unwrap();
    let end = finite.then_some(Timestamp::from_nanos(4_000_000_010));
    let mut capture = if selected || builtin {
        prepare_section_capture_for_policy(
            &source,
            &judge,
            &policy,
            ClockDomainId(17),
            Timestamp::ZERO,
            0,
            end,
            Some(replay_limits().unwrap()),
        )
        .unwrap()
        .unwrap()
    } else {
        LiveReplayCapture::new_with_policy(
            &judge,
            ClockDomainId(17),
            replay_limits().unwrap(),
            Timestamp::ZERO,
            0,
            end,
            BmsInputMode::ButtonOnly,
            None,
            &policy,
        )
        .unwrap()
    };
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
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let mut score = ScoreSummary::default();
    let mut gauge = BmsGauge::new(policy.gauge().clone());
    let mut prefix_len = 0;
    for index in 0..4 {
        let ns = index * 1_000_000_000 + index + 1;
        for (offset, state) in [ButtonState::Down, ButtonState::Up].into_iter().enumerate() {
            let report = runtime
                .process_input(
                    PhysicalInputEvent::Button(ButtonEvent {
                        meta: EventMeta::new(
                            DeviceId(9),
                            point(ns),
                            index as u64 * 2 + offset as u64,
                        ),
                        control: PhysicalControlId::keyboard(7u16),
                        state,
                    }),
                    &Identity,
                    point(ns),
                )
                .unwrap();
            assert!(report.judge_error.is_none());
            score.observe(&report.judge_events).unwrap();
            gauge
                .observe(&report.judge_events, &report.hazard_events)
                .unwrap();
            capture.record_report(&report).unwrap();
        }
        if index == 0 {
            assert_eq!((score.hits, score.misses), (1, 0));
            prefix_len = capture.records().len();
        }
    }
    let report = runtime
        .advance_to(point(4_000_000_010), &Identity, point(4_000_000_010))
        .unwrap();
    score.observe(&report.judge_events).unwrap();
    gauge
        .observe(&report.judge_events, &report.hazard_events)
        .unwrap();
    capture.record_report(&report).unwrap();
    assert_eq!((score.hits, score.misses), (4, 1));
    let result = CompletedPlayResult::from_completed(Timestamp::ZERO, end, &gauge);
    let bytes = capture.into_bytes().unwrap();
    let mut file = decode_replay(&bytes, replay_limits().unwrap()).unwrap();
    assert_eq!(
        encode_replay(&file, replay_limits().unwrap()).unwrap(),
        bytes
    );
    // A valid captured prefix shares the final archive's exact selected identity.
    file.records.truncate(prefix_len);
    (file, score, result, policy.gauge().clone())
}
fn archive(
    file: &ReplayFile,
    score: Option<&ScoreSummary>,
    result: CompletedPlayResult,
    profile: GaugeProfile,
) -> ResultArchive {
    let rows = [(PlayerId(7), result)];
    let identities = [(PlayerId(7), file.header.clone(), profile)];
    match score {
        Some(score) => {
            ResultArchive::from_completed_with_scores(&rows, &identities, &[(PlayerId(7), score)])
                .unwrap()
        }
        None => ResultArchive::from_completed(&rows, &identities).unwrap(),
    }
}
fn preview(
    file: ReplayFile,
    archive: Option<&ResultArchive>,
    finite: bool,
    selected: bool,
) -> RecordPreview {
    let mut args: Vec<String> = ["--early-ns", "4", "--late-ns", "4"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    if selected {
        args.extend(["--gauge".into(), "groove".into()]);
    }
    if finite {
        args.extend(["--end-ns".into(), "4000000010".into()]);
    }
    let settings = NativeSettings::from_args(&args, SettingsHost::Linux).unwrap();
    RecordPreview::from_file_with_archive(
        Path::new("memory.bkr"),
        &source(),
        &settings,
        file,
        archive,
        archive.map(|_| PlayerId(7)),
    )
    .unwrap()
}
fn frame<'a>(
    directory: &'a LineEditor,
    catalog: &'a RecordCatalog,
    preview: &'a RecordPreview,
    details: bool,
) -> RecordsFrame<'a> {
    RecordsFrame {
        directory,
        directory_focused: false,
        catalog: Some(catalog),
        selected: Some(0),
        first: 0,
        preview: Some(preview),
        pending: false,
        details,
        grade_page: 0,
        opponents: 0,
        selected_opponents: [0; 2],
        message: None,
        error: None,
        hovered: None,
        armed: None,
    }
}
fn catalog(preview: &RecordPreview) -> RecordCatalog {
    RecordCatalog {
        entries: vec![preview.path.clone()],
        truncated: false,
    }
}
type Packet = Vec<([f32; 4], [f32; 4], [f32; 4])>;
fn packet(scene: &Scene) -> Packet {
    scene
        .rectangles()
        .iter()
        .map(|rectangle| (rectangle.bounds, rectangle.uv, rectangle.color))
        .collect()
}
fn compose(view: &RecordsView) -> (Scene, Vec<(ControlId, Bounds)>) {
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    assert!(!view.dirty());
    (scene, hits)
}
/// Check complete labels and exact glyph positions, excluding unrelated rows.
fn label(scene: &Scene, x: f32, y: f32, value: &str) {
    for (index, character) in value.chars().enumerate() {
        assert!(
            scene.rectangles().iter().any(|rectangle| {
                rectangle.bounds == [x + index as f32 * 6.0, y, 5.0, 7.0]
                    && rectangle.uv == crate::font::glyph_uv(character)
            }),
            "missing {value:?} glyph {index} at ({x}, {y})"
        );
    }
}
fn stored_labels(scene: &Scene, score: BmsScoreSummary) {
    for (y, name, value) in [
        (322., "EX", score.ex_score),
        (346., "PGREAT", score.pgreat),
        (370., "GREAT", score.great),
        (394., "GOOD", score.good),
        (418., "BAD", score.bad),
        (442., "POOR", score.poor),
    ] {
        label(scene, 500., y, &format!("STORED {name} {value}"));
    }
}

fn catalog_class_rows_do_not_overlap(scene: &Scene) {
    label(scene, 24., 472., "1 RECORDS");
    let mut spans = Vec::new();
    for y in [472., 482., 492., 502.] {
        let glyphs: Vec<_> = scene
            .rectangles()
            .iter()
            .filter(|rectangle| {
                rectangle.bounds[0] < 620. && rectangle.bounds[1] == y && rectangle.bounds[3] == 7.
            })
            .collect();
        assert!(!glyphs.is_empty(), "missing catalog/class row at {y}");
        let top = glyphs
            .iter()
            .map(|rectangle| rectangle.bounds[1])
            .fold(f32::INFINITY, f32::min);
        let bottom = glyphs
            .iter()
            .map(|rectangle| rectangle.bounds[1] + rectangle.bounds[3])
            .fold(f32::NEG_INFINITY, f32::max);
        spans.push((top, bottom));
    }
    for pair in spans.windows(2) {
        assert!(
            pair[0].1 <= pair[1].0,
            "catalog summary and class rows overlap: {pair:?}"
        );
    }
}

#[test]
fn selected_native_prefix_and_associated_final_are_distinct_through_public_retained_ui() {
    for finite in [false, true] {
        let (file, score, result, profile) = actual_capture(true, finite);
        let archive = archive(&file, Some(&score), result, profile);
        let archive = decode_archive(&encode_archive(&archive).unwrap()).unwrap();
        let preview = preview(file.clone(), Some(&archive), finite, true);
        let prefix = BmsScoreSummary {
            pgreat: 1,
            ex_score: 2,
            ..Default::default()
        };
        let stored = BmsScoreSummary {
            pgreat: 4,
            poor: 1,
            ex_score: 8,
            ..Default::default()
        };
        assert_eq!((preview.score.hits, preview.score.misses), (1, 0));
        assert_eq!(preview.bms_score, Some(prefix));
        assert_eq!(preview.historical_bms_score, Some(stored));
        assert_eq!(preview.historical_score.as_ref().unwrap().hits, 4);
        let directory = LineEditor::new("records", 4096).unwrap();
        let catalog = catalog(&preview);
        let view = RecordsView::new(ScreenInstanceId(1401), 960, 720).unwrap();
        view.update(frame(&directory, &catalog, &preview, false))
            .unwrap();
        let (scene, _) = compose(&view);
        label(&scene, 24., 482., "PREFIX EX 2");
        label(&scene, 24., 492., "PGREAT 1 GREAT 0");
        label(&scene, 24., 502., "GOOD 0 BAD 0 POOR 0");
        catalog_class_rows_do_not_overlap(&scene);
        view.update(frame(&directory, &catalog, &preview, true))
            .unwrap();
        let (scene, hits) = compose(&view);
        stored_labels(&scene, stored);
        assert!(scene.playfields().is_empty());
        assert!(hits.iter().any(|(id, _)| *id == ControlId(66)));
        let bytes = encode_replay(&file, replay_limits().unwrap()).unwrap();
        let stored_bytes = encode_archive(&archive).unwrap();
        let presentation =
            HistoricalRecordPresentation::new(&bytes, Some(&stored_bytes), Some(PlayerId(7)))
                .unwrap()
                .unwrap();
        assert_eq!(presentation.bms_score(), Some(stored));
        let mut historical_scene = Scene::new(960, 720);
        presentation.compose(&mut historical_scene).unwrap();
        stored_labels(&historical_scene, stored);
        // Association failure cannot erase the valid classified Runtime prefix.
        let mut foreign = file.clone();
        foreign.header.seed ^= 1;
        let foreign = archive_with_score(
            &foreign,
            &score,
            result,
            archive.entries()[0].profile.clone(),
        );
        let rejected = preview_from(file, Some(&foreign), finite);
        assert_eq!(rejected.bms_score, Some(prefix));
        assert!(rejected.historical_bms_score.is_none());
        assert!(rejected.historical_score.is_none());
        assert!(rejected.historical.is_none());
        assert!(rejected.archive_error.is_some());
        view.update(frame(&directory, &catalog, &rejected, false))
            .unwrap();
        let (scene, _) = compose(&view);
        label(&scene, 24., 482., "PREFIX EX 2");
    }
}

fn archive_with_score(
    file: &ReplayFile,
    score: &ScoreSummary,
    result: CompletedPlayResult,
    profile: GaugeProfile,
) -> ResultArchive {
    archive(file, Some(score), result, profile)
}
fn preview_from(file: ReplayFile, archive: Option<&ResultArchive>, finite: bool) -> RecordPreview {
    preview(file, archive, finite, true)
}

#[test]
fn actual_all_class_runtime_archive_projects_through_historical_bytes_and_legacy_is_unavailable() {
    let (file, score, result, profile) = actual_capture(false, false);
    let archive = archive(&file, Some(&score), result, profile);
    let replay = encode_replay(&file, replay_limits().unwrap()).unwrap();
    let bytes = encode_archive(&archive).unwrap();
    let presentation = HistoricalRecordPresentation::new(&replay, Some(&bytes), Some(PlayerId(7)))
        .unwrap()
        .unwrap();
    let expected = BmsScoreSummary {
        pgreat: 1,
        great: 1,
        good: 1,
        bad: 1,
        poor: 1,
        ex_score: 3,
    };
    assert_eq!(presentation.bms_score(), Some(expected));
    let mut scene = Scene::new(960, 720);
    presentation.compose(&mut scene).unwrap();
    stored_labels(&scene, expected);
    let original = packet(&scene);
    scene.clear();
    presentation.compose(&mut scene).unwrap();
    assert_eq!(packet(&scene), original);
    // Existing constructors deliberately lack recorded class identity.
    let entry = &archive.entries()[0];
    let legacy = HistoricalRecordPresentation::from_record(
        (entry.player, entry.result),
        entry.score.as_ref(),
    )
    .unwrap();
    assert_eq!(legacy.bms_score(), None);
    scene.clear();
    legacy.compose(&mut scene).unwrap();
    label(&scene, 500., 322., "STORED CLASS SCORE UNAVAILABLE");
}

#[test]
fn missing_and_unclassified_score_never_render_a_guessed_zero_class_score() {
    let (file, _, result, profile) = actual_capture(true, false);
    let missing = archive(&file, None, result, profile);
    let missing = preview(file.clone(), Some(&missing), false, true);
    assert_eq!(missing.bms_score.unwrap().ex_score, 2);
    assert!(missing.historical_bms_score.is_none());
    let directory = LineEditor::new("records", 4096).unwrap();
    let catalog = catalog(&missing);
    let view = RecordsView::new(ScreenInstanceId(1402), 960, 720).unwrap();
    view.update(frame(&directory, &catalog, &missing, true))
        .unwrap();
    let (scene, _) = compose(&view);
    label(&scene, 500., 322., "STORED CLASS SCORE UNAVAILABLE");
    // A genuine builtin capture matches the legacy draft's whole policy.
    // Removing only class metadata from a selected BMS policy does not.
    let mut stripped = file;
    let (inner, _) =
        crate::replay_judgment_policy::split_options(&stripped.header.options).unwrap();
    stripped.header.options = inner.to_vec();
    let builtin_settings = NativeSettings::from_args(
        &[
            "--early-ns".into(),
            "4".into(),
            "--late-ns".into(),
            "4".into(),
        ],
        SettingsHost::Linux,
    )
    .unwrap();
    assert!(
        RecordPreview::from_file_with_archive(
            Path::new("memory.bkr"),
            &source(),
            &builtin_settings,
            stripped,
            None,
            None,
        )
        .is_err(),
        "removing classes must not bypass native gauge compatibility"
    );
    let (file, score, result, profile) = actual_capture_with_policy(false, false, true);
    let legacy = archive(&file, Some(&score), result, profile);
    let legacy = preview(file.clone(), Some(&legacy), false, false);
    assert_eq!(legacy.score.hits, 1);
    assert!(legacy.bms_score.is_none());
    assert!(legacy.historical_bms_score.is_none());
    view.update(frame(&directory, &catalog, &legacy, false))
        .unwrap();
    let (scene, _) = compose(&view);
    label(&scene, 24., 482., "PREFIX CLASS SCORE UNAVAILABLE");
    view.update(frame(&directory, &catalog, &legacy, true))
        .unwrap();
    let (scene, _) = compose(&view);
    label(&scene, 500., 322., "STORED CLASS SCORE UNAVAILABLE");
    let absent = preview(file, None, false, false);
    assert!(absent.bms_score.is_none());
    assert!(absent.historical_bms_score.is_none());
}

#[test]
fn scalar_class_changes_invalidate_both_modes_with_unchanged_generic_score_and_arc() {
    let (file, score, result, profile) = actual_capture(true, false);
    let archive = archive(&file, Some(&score), result, profile);
    let mut preview = preview(file, Some(&archive), false, true);
    let generic = preview.score.clone();
    let stored_arc = preview.historical_score.as_ref().unwrap().clone();
    let directory = LineEditor::new("records", 4096).unwrap();
    let catalog = catalog(&preview);
    for details in [false, true] {
        let view = RecordsView::new(ScreenInstanceId(1403), 960, 720).unwrap();
        view.update(frame(&directory, &catalog, &preview, details))
            .unwrap();
        let (scene, _) = compose(&view);
        let original = packet(&scene);
        for _ in 0..3 {
            view.update(frame(&directory, &catalog, &preview, details))
                .unwrap();
            assert!(!view.dirty());
            assert_eq!(packet(&compose(&view).0), original);
        }
        if details {
            preview.historical_bms_score = Some(BmsScoreSummary {
                great: 4,
                poor: 1,
                ex_score: 4,
                ..Default::default()
            });
        } else {
            preview.bms_score = Some(BmsScoreSummary {
                great: 1,
                ex_score: 1,
                ..Default::default()
            });
        }
        assert_eq!(preview.score, generic);
        assert!(Arc::ptr_eq(
            preview.historical_score.as_ref().unwrap(),
            &stored_arc
        ));
        view.update(frame(&directory, &catalog, &preview, details))
            .unwrap();
        assert!(view.dirty());
        let (scene, _) = compose(&view);
        assert_ne!(packet(&scene), original);
        if details {
            stored_labels(&scene, preview.historical_bms_score.unwrap());
        } else {
            label(&scene, 24., 482., "PREFIX EX 1");
            label(&scene, 24., 492., "PGREAT 0 GREAT 1");
        }
        let changed = packet(&scene);
        view.update(frame(&directory, &catalog, &preview, details))
            .unwrap();
        assert!(!view.dirty());
        assert_eq!(packet(&compose(&view).0), changed);
    }
}

#[test]
fn malformed_class_math_poor_sums_and_overflow_refuse_before_retained_scene_changes() {
    let (file, score, result, profile) = actual_capture(true, false);
    let archive = archive(&file, Some(&score), result, profile);
    let preview = preview(file, Some(&archive), false, true);
    let directory = LineEditor::new("records", 4096).unwrap();
    let catalog = catalog(&preview);
    for details in [false, true] {
        let view = RecordsView::new(ScreenInstanceId(1404), 960, 720).unwrap();
        view.update(frame(&directory, &catalog, &preview, details))
            .unwrap();
        let original = packet(&compose(&view).0);
        for field in 0..4 {
            let mut bad = preview.clone();
            let classes = if details {
                &mut bad.historical_bms_score
            } else {
                &mut bad.bms_score
            };
            let classes = classes.as_mut().unwrap();
            match field {
                0 => classes.ex_score += 1,
                1 => classes.poor += 1,
                2 => classes.good += 1,
                _ => {
                    classes.pgreat = u64::MAX;
                    classes.great = 1;
                }
            }
            assert!(
                view.update(frame(&directory, &catalog, &bad, details))
                    .is_err(),
                "mode {details}, field {field}"
            );
            assert!(!view.dirty());
            assert_eq!(packet(&compose(&view).0), original);
            if details {
                assert!(
                    HistoricalRecordPresentation::from_record_with_class_score(
                        bad.historical.unwrap(),
                        bad.historical_score.as_deref(),
                        None,
                        bad.historical_bms_score,
                    )
                    .is_err()
                );
            }
        }
        // EX multiplication overflow must refuse even when class sum matches hits.
        let mut bad = preview.clone();
        let n = u64::MAX / 2 + 1;
        let classes = BmsScoreSummary {
            pgreat: n,
            ex_score: 0,
            ..Default::default()
        };
        if details {
            bad.historical_score = Some(Arc::new(ArchivedScore {
                hits: n,
                misses: 0,
                combo: 0,
                max_combo: 0,
                grades: vec![(0, n)],
                timing: Default::default(),
            }));
            bad.historical_bms_score = Some(classes);
        } else {
            bad.score = ScoreSummary {
                hits: n,
                grades: [(0, n)].into_iter().collect(),
                ..Default::default()
            };
            bad.bms_score = Some(classes);
        }
        assert!(
            view.update(frame(&directory, &catalog, &bad, details))
                .is_err()
        );
        assert!(!view.dirty());
        assert_eq!(packet(&compose(&view).0), original);
    }
}

#[test]
fn valid_u64_extremes_stay_inside_columns_and_clear_all_button_rectangles() {
    let (file, score, result, profile) = actual_capture(true, false);
    let archive = archive(&file, Some(&score), result, profile);
    let mut preview = preview(file, Some(&archive), false, true);
    // Scalar display boundary injection; primary provenance is tested above.
    let directory = LineEditor::new("records", 4096).unwrap();
    let catalog = catalog(&preview);
    let view = RecordsView::new(ScreenInstanceId(1405), 960, 720).unwrap();
    for (hits, misses, classes) in [
        (
            u64::MAX,
            0,
            BmsScoreSummary {
                great: u64::MAX,
                ex_score: u64::MAX,
                ..Default::default()
            },
        ),
        (
            u64::MAX,
            0,
            BmsScoreSummary {
                good: u64::MAX,
                ..Default::default()
            },
        ),
        (
            0,
            u64::MAX,
            BmsScoreSummary {
                poor: u64::MAX,
                ..Default::default()
            },
        ),
    ] {
        classes.validate_for(hits, misses).unwrap();
        preview.score = ScoreSummary {
            hits,
            misses,
            grades: if hits == 0 {
                Default::default()
            } else {
                [(0, hits)].into_iter().collect()
            },
            ..Default::default()
        };
        preview.bms_score = Some(classes);
        preview.historical_score = Some(Arc::new(
            ArchivedScore::from_summary(&preview.score).unwrap(),
        ));
        preview.historical_bms_score = Some(classes);
        for details in [false, true] {
            view.update(frame(&directory, &catalog, &preview, details))
                .unwrap();
            let (scene, hits) = compose(&view);
            if details {
                stored_labels(&scene, classes);
            } else {
                label(
                    &scene,
                    24.,
                    482.,
                    &format!("PREFIX EX {}", classes.ex_score),
                );
                label(
                    &scene,
                    24.,
                    492.,
                    &format!("PGREAT {} GREAT {}", classes.pgreat, classes.great),
                );
                label(
                    &scene,
                    24.,
                    502.,
                    &format!("GOOD {} BAD 0 POOR {}", classes.good, classes.poor),
                );
            }
            let rows: &[f32] = if details {
                &[322., 346., 370., 394., 418., 442.]
            } else {
                &[482., 492., 502.]
            };
            for rectangle in scene.rectangles().iter().filter(|rectangle| {
                rows.contains(&rectangle.bounds[1]) && rectangle.bounds[3] == 7.
            }) {
                let [x, y, width, height] = rectangle.bounds;
                assert!(x >= 0. && x + width <= 960. && y + height <= 720.);
                if details && x < 500. {
                    assert!(
                        x + width <= 476.,
                        "generic stored score must stay in left column"
                    );
                } else if !details {
                    assert!(
                        x + width <= 620.,
                        "prefix class text must clear page controls"
                    );
                }
                for (_, button) in &hits {
                    let bx = button.x as f32;
                    let by = button.y as f32;
                    assert!(
                        x + width <= bx
                            || x >= bx + button.width as f32
                            || y + height <= by
                            || y >= by + button.height as f32,
                        "class glyph overlaps a button"
                    );
                }
            }
        }
    }
}
