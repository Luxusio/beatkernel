//! Original records models exercise the mounted screen without a GPU or IO owner.
use super::*;
use crate::{
    competition::{OpponentKind, ScoreSummary},
    competition_presentation::{
        CompetitionSnapshot, GhostSnapshot, NetworkSnapshot, NetworkStatus,
    },
    gauge::BmsGauge,
    local_players::PlayerId,
    play_result::{PlayResultOutcome, PlayResultScope},
    result_archive::{ArchivedResult, ArchivedScore},
    ui::layout::{LayoutChange, LayoutUpdate, NodeId},
};

fn catalog(count: usize) -> RecordCatalog {
    RecordCatalog {
        entries: (0..count)
            .map(|i| format!("records/{i}.bkr").into())
            .collect(),
        truncated: count == 256,
    }
}
fn preview(path: &std::path::Path) -> RecordPreview {
    let stored = ScoreSummary {
        hits: 9,
        max_combo: 9,
        grades: (0..9).map(|grade| (grade, 1)).collect(),
        ..Default::default()
    };
    RecordPreview {
        path: path.into(),
        records: 3,
        recorded_until: Some(Timestamp::from_nanos(2_000_000_000)),
        start: Timestamp::ZERO,
        end: Some(Timestamp::from_nanos(3_000_000_000)),
        historical: Some((
            PlayerId(u32::MAX),
            ArchivedResult {
                scope: PlayResultScope::PracticeSection {
                    start: Timestamp::ZERO,
                    end: Some(Timestamp::from_nanos(3_000_000_000)),
                },
                outcome: PlayResultOutcome::BelowClearThreshold,
                gauge: *BmsGauge::default().snapshot(),
            },
        )),
        historical_score: Some(Arc::new(ArchivedScore::from_summary(&stored).unwrap())),
        bms_score: None,
        historical_bms_score: None,
        historical_comparison: Some(Arc::new(Some(CompetitionSnapshot {
            ghosts: vec![GhostSnapshot {
                kind: OpponentKind::Own,
                label: "original-own.bkr".into(),
                hits: 4,
                misses: 1,
                combo: 2,
                max_combo: 4,
                recorded_until: None,
            }],
            network: Some(NetworkSnapshot {
                status: NetworkStatus::Stopped,
                progress: None,
            }),
        }))),
        archive_error: None,
        score: ScoreSummary {
            hits: 2,
            misses: 1,
            max_combo: 2,
            ..Default::default()
        },
    }
}
fn frame<'a>(
    directory: &'a LineEditor,
    catalog: &'a RecordCatalog,
    preview: Option<&'a RecordPreview>,
) -> RecordsFrame<'a> {
    RecordsFrame {
        directory,
        directory_focused: true,
        catalog: Some(catalog),
        selected: (!catalog.entries.is_empty()).then_some(0),
        first: 0,
        preview,
        pending: false,
        details: false,
        grade_page: 0,
        opponents: 0,
        selected_opponents: [0; 2],
        message: None,
        error: None,
        hovered: None,
        armed: None,
    }
}
fn compose(view: &RecordsView, width: u32, height: u32) -> (Scene, Vec<(ControlId, Bounds)>) {
    let mut scene = Scene::new(width, height);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    if width != 0 && height != 0 {
        scene.status().unwrap();
    }
    (scene, hits)
}
fn label(scene: &Scene, label: &str) {
    let expected: Vec<_> = label.chars().map(crate::font::glyph_uv).collect();
    let actual: Vec<_> = scene.rectangles().iter().map(|r| r.uv).collect();
    assert!(
        actual
            .windows(expected.len())
            .any(|window| window == expected),
        "missing records caption {label}"
    );
}
fn packet(scene: &Scene) -> Vec<([f32; 4], [f32; 4], [f32; 4])> {
    scene
        .rectangles()
        .iter()
        .map(|r| (r.bounds, r.uv, r.color))
        .collect()
}
fn row_node(view: &RecordsView, slot: usize) -> NodeId {
    view.layout
        .borrow()
        .leaves()
        .iter()
        .find(|leaf| matches!(leaf.component, Component::Row(index) if index == slot))
        .unwrap()
        .id
}

fn hit_packet(hits: &[(ControlId, Bounds)]) -> Vec<(u64, [i64; 4])> {
    hits.iter()
        .map(|(id, b)| (id.0, [b.x, b.y, b.width, b.height]))
        .collect()
}

fn hovered_detail_frame<'a>(
    directory: &'a LineEditor,
    catalog: &'a RecordCatalog,
    preview: &'a RecordPreview,
    page: usize,
) -> RecordsFrame<'a> {
    let mut update = frame(directory, catalog, Some(preview));
    update.details = true;
    update.grade_page = page;
    update.hovered = Some(ControlId(67));
    update.armed = Some(ControlId(67));
    update
}

fn assert_hidden_records_surfaces(view: &RecordsView, details: bool) {
    let mut scene = Scene::new(960, 720);
    scene.rect(0, 0, 960, 720, 0);
    let mut hits = vec![(
        ControlId(999),
        Bounds {
            x: 0,
            y: 0,
            width: 960,
            height: 720,
        },
    )];
    view.compose(&mut scene, &mut hits).unwrap();
    assert!(scene.rectangles().is_empty());
    assert!(hits.is_empty());
    let animated = node_id(
        &view.layout.borrow(),
        if details {
            Component::DetailButton(0)
        } else {
            Component::Directory
        },
    );
    view.compose_components(&mut scene, &mut hits, view.id(), &[animated])
        .unwrap();
    assert!(scene.rectangles().is_empty());
    assert!(hits.is_empty());
    for point in [
        (30.0, 180.0),
        (170.0, 115.0),
        (755.0, 621.0),
        (431.0, 576.0),
    ] {
        assert_eq!(view.hit(point), None);
        assert_eq!(view.hit_components(&scene, view.id(), point), None);
    }
}

fn assert_cached_metadata(view: &RecordsView, preview: &RecordPreview) -> *const GhostSnapshot {
    let cache = view.detail_cache.borrow();
    let cache = cache.as_ref().unwrap();
    assert!(Arc::ptr_eq(
        cache.score.as_ref().unwrap(),
        preview.historical_score.as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        cache.comparison.as_ref().unwrap(),
        preview.historical_comparison.as_ref().unwrap()
    ));
    assert_eq!(
        cache.presentation.comparison(),
        Some(preview.historical_comparison.as_ref().unwrap().as_ref())
    );
    cache
        .presentation
        .comparison()
        .unwrap()
        .as_ref()
        .unwrap()
        .ghosts
        .as_ptr()
}

fn assert_restored_component_surface(
    view: &RecordsView,
    details: bool,
    expected: &Scene,
    expected_hits: &[(ControlId, Bounds)],
) {
    use crate::scene::UiComponentKey;
    let animated = node_id(
        &view.layout.borrow(),
        if details {
            Component::DetailButton(0)
        } else {
            Component::Directory
        },
    );
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose_components(&mut scene, &mut hits, view.id(), &[animated])
        .unwrap();
    assert_eq!(packet(&scene), packet(expected));
    assert_eq!(hit_packet(&hits), hit_packet(expected_hits));
    assert!(scene
        .component_id(UiComponentKey {
            screen: view.id(),
            node: animated
        })
        .is_some());
    for (id, b) in hits {
        let point = (b.x as f64 + 1.0, b.y as f64 + 1.0);
        assert_eq!(view.hit_components(&scene, view.id(), point), Some(id));
        assert_eq!(
            view.hit_components(&scene, ScreenInstanceId(view.id().0 + 100), point),
            None
        );
    }
}

#[test]
fn initial_zero_chooser_and_historical_pages_restore_exact_active_surface_and_actions() {
    let catalog = catalog(1);
    let directory = LineEditor::new("records", 4096).unwrap();
    let preview = preview(&catalog.entries[0]);
    for extent in [[0, 0], [0, 720], [960, 0]] {
        for page in [None, Some(0), Some(3), Some(4)] {
            let owner = ScreenInstanceId(1250);
            let view = RecordsView::new(owner, extent[0], extent[1]).unwrap();
            let reference = RecordsView::new(ScreenInstanceId(1251), 960, 720).unwrap();
            let update = || {
                let mut update = frame(&directory, &catalog, Some(&preview));
                update.details = page.is_some();
                update.grade_page = page.unwrap_or(0);
                update
            };
            view.update(update()).unwrap();
            reference.update(update()).unwrap();
            let identities = view.nodes.identities();
            let mounted = view
                .layout
                .borrow()
                .leaves()
                .iter()
                .map(|leaf| leaf.id)
                .collect::<Vec<_>>();
            let presenter = page.map(|_| assert_cached_metadata(&view, &preview));
            assert_hidden_records_surfaces(&view, page.is_some());
            view.resize(960, 720).unwrap();
            let (actual, actual_hits) = compose(&view, 960, 720);
            let (expected, expected_hits) = compose(&reference, 960, 720);
            assert_eq!(packet(&actual), packet(&expected));
            assert_eq!(hit_packet(&actual_hits), hit_packet(&expected_hits));
            assert_restored_component_surface(&view, page.is_some(), &expected, &expected_hits);
            assert_eq!(view.id(), owner);
            assert_eq!(view.nodes.identities(), identities);
            assert_eq!(
                view.layout
                    .borrow()
                    .leaves()
                    .iter()
                    .map(|leaf| leaf.id)
                    .collect::<Vec<_>>(),
                mounted
            );
            if let Some(pointer) = presenter {
                assert_eq!(assert_cached_metadata(&view, &preview), pointer);
            }
            for (id, b) in actual_hits {
                assert_eq!(view.hit((b.x as f64 + 1.0, b.y as f64 + 1.0)), Some(id));
            }
        }
    }
}

#[test]
fn hidden_chooser_editor_font_catalog_pending_and_hover_restore_latest_reference_scene() {
    use crate::{font_atlas::FontAtlas, texture::TextureId};
    let mut atlas = FontAtlas::new(crate::font_fixture::font_bytes(), 14.0, 128, 128, 16).unwrap();
    atlas.prepare('A').unwrap();
    atlas.prepare('가').unwrap();
    let texture = TextureId::allocate().unwrap();
    let font = FontText::new(Arc::new(atlas), texture).unwrap();
    let old_catalog = catalog(1);
    let old_directory = LineEditor::new("records", 4096).unwrap();
    let latest_catalog = catalog(25);
    let directory = LineEditor::new("가A", 4096).unwrap();
    let preview = preview(&latest_catalog.entries[11]);
    let view = RecordsView::new(ScreenInstanceId(1252), 960, 720).unwrap();
    view.update(frame(&old_directory, &old_catalog, None))
        .unwrap();
    compose(&view, 960, 720);
    let identities = view.nodes.identities();
    view.resize(0, 720).unwrap();
    view.set_input_font(Some(font.clone()));
    let latest = |pending| {
        let mut update = frame(&directory, &latest_catalog, Some(&preview));
        update.first = 10;
        update.selected = Some(11);
        update.pending = pending;
        update.hovered = Some(ControlId(51));
        update.armed = Some(ControlId(51));
        update.message = Some("LATEST CATALOG");
        update
    };
    view.update(latest(true)).unwrap();
    assert_hidden_records_surfaces(&view, false);
    view.update(latest(false)).unwrap();
    assert_hidden_records_surfaces(&view, false);
    view.resize(960, 720).unwrap();
    let reference = RecordsView::new(ScreenInstanceId(1253), 960, 720).unwrap();
    reference.set_input_font(Some(font));
    reference.update(latest(false)).unwrap();
    let (actual, hits) = compose(&view, 960, 720);
    let (expected, expected_hits) = compose(&reference, 960, 720);
    assert_eq!(packet(&actual), packet(&expected));
    assert_eq!(hit_packet(&hits), hit_packet(&expected_hits));
    assert_restored_component_surface(&view, false, &expected, &expected_hits);
    assert!(actual
        .batches()
        .iter()
        .any(|batch| batch.texture == texture));
    label(&actual, "LATEST CATALOG");
    assert!(hits.iter().any(|(id, _)| *id == ControlId(50011)));
    assert_eq!(view.nodes.identities(), identities);
}

#[test]
fn hidden_detail_page_hover_and_replacement_metadata_preserve_matching_cache_and_reject_candidates()
{
    let catalog = catalog(1);
    let directory = LineEditor::new("records", 4096).unwrap();
    let original = preview(&catalog.entries[0]);
    let view = RecordsView::new(ScreenInstanceId(1254), 960, 720).unwrap();
    view.update(hovered_detail_frame(&directory, &catalog, &original, 0))
        .unwrap();
    compose(&view, 960, 720);
    let pointer = assert_cached_metadata(&view, &original);
    let identities = view.nodes.identities();
    for extent in [[0, 0], [0, 720], [960, 0]] {
        view.resize(extent[0], extent[1]).unwrap();
        for page in [1, 3, 4] {
            view.update(hovered_detail_frame(&directory, &catalog, &original, page))
                .unwrap();
            assert_eq!(assert_cached_metadata(&view, &original), pointer);
            assert_hidden_records_surfaces(&view, true);
        }
    }
    let mut replacement = preview(&catalog.entries[0]);
    let score = ScoreSummary {
        hits: 12,
        max_combo: 12,
        grades: [(42, 12)].into(),
        ..Default::default()
    };
    replacement.historical_score = Some(Arc::new(ArchivedScore::from_summary(&score).unwrap()));
    replacement.historical_comparison = Some(Arc::new(Some(CompetitionSnapshot {
        ghosts: vec![GhostSnapshot {
            label: "new-hidden-own.bkr".into(),
            ..original
                .historical_comparison
                .as_ref()
                .unwrap()
                .as_ref()
                .as_ref()
                .unwrap()
                .ghosts[0]
                .clone()
        }],
        network: Some(NetworkSnapshot {
            status: NetworkStatus::Stopped,
            progress: None,
        }),
    })));
    view.update(hovered_detail_frame(&directory, &catalog, &replacement, 1))
        .unwrap();
    let replacement_pointer = assert_cached_metadata(&view, &replacement);
    assert_ne!(replacement_pointer, pointer);
    let mut invalid = hovered_detail_frame(&directory, &catalog, &replacement, 1);
    invalid.pending = true;
    assert!(view.update(invalid).is_err());
    let mut invalid = hovered_detail_frame(&directory, &catalog, &replacement, usize::MAX);
    invalid.selected = Some(1);
    assert!(view.update(invalid).is_err());
    let revision = view.layout.borrow().revision();
    assert!(view
        .update_layout(&[LayoutUpdate {
            id: NodeId(usize::MAX),
            change: LayoutChange::Size([1, 1])
        }])
        .is_err());
    assert_eq!(view.layout.borrow().revision(), revision);
    assert_eq!(
        assert_cached_metadata(&view, &replacement),
        replacement_pointer
    );
    assert_hidden_records_surfaces(&view, true);
    view.resize(960, 720).unwrap();
    let reference = RecordsView::new(ScreenInstanceId(1255), 960, 720).unwrap();
    reference
        .update(hovered_detail_frame(&directory, &catalog, &replacement, 1))
        .unwrap();
    let (actual, hits) = compose(&view, 960, 720);
    let (expected, expected_hits) = compose(&reference, 960, 720);
    assert_eq!(packet(&actual), packet(&expected));
    assert_eq!(hit_packet(&hits), hit_packet(&expected_hits));
    assert_restored_component_surface(&view, true, &expected, &expected_hits);
    label(&actual, "SAVED REPLAY OPERATION PREFIX");
    label(&actual, "new-hidden-own.bkr");
    assert_eq!(view.nodes.identities(), identities);
    assert_eq!(
        assert_cached_metadata(&view, &replacement),
        replacement_pointer
    );
}

#[test]
fn mounted_catalog_keeps_original_controls_prefix_and_stored_meanings_in_painter_order() {
    let catalog = catalog(25);
    let directory = LineEditor::new("records", 4096).unwrap();
    let preview = preview(&catalog.entries[10]);
    let view = RecordsView::new(ScreenInstanceId(1201), 960, 720).unwrap();
    let mut update = frame(&directory, &catalog, Some(&preview));
    update.first = 10;
    update.selected = Some(10);
    update.opponents = 3;
    update.selected_opponents = [2, 1];
    view.update(update).unwrap();
    let (scene, hits) = compose(&view, 960, 720);
    assert_eq!(
        hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
        std::iter::once(58)
            .chain(50010..50020)
            .chain([56, 57, 50, 51, 52, 53, 54, 55, 59, 60, 61, 66])
            .collect::<Vec<_>>()
    );
    label(&scene, "HITS 2 MISSES 1");
    label(&scene, "HISTORICAL PLAYER 4294967295");
    label(&scene, "STORED SCOPE PRACTICE SECTION");
    label(&scene, "25 RECORDS");
    for (id, bounds) in &hits {
        let point = (bounds.x as f64 + 1.0, bounds.y as f64 + 1.0);
        assert_eq!(view.hit(point), Some(*id));
    }
    let identities = view.nodes.identities();
    let paints = view.nodes.paints();
    let mut same = frame(&directory, &catalog, Some(&preview));
    same.first = 10;
    same.selected = Some(10);
    same.opponents = 3;
    same.selected_opponents = [2, 1];
    view.update(same).unwrap();
    assert!(!view.dirty());
    assert_eq!(view.nodes.identities(), identities);
    assert_eq!(view.nodes.paints(), paints);
    assert_eq!(packet(&compose(&view, 960, 720).0), packet(&scene));
}

#[test]
fn mounted_archive_pages_preserve_grade_and_comparison_data_and_back_isolated_from_catalog() {
    let catalog = catalog(1);
    let directory = LineEditor::new("records", 4096).unwrap();
    let preview = preview(&catalog.entries[0]);
    let score = preview.historical_score.as_ref().unwrap().clone();
    let comparison = preview.historical_comparison.as_ref().unwrap().clone();
    let view = RecordsView::new(ScreenInstanceId(1202), 960, 720).unwrap();
    let mut presenter_pointer = None;
    for page in 0..5 {
        let mut update = frame(&directory, &catalog, Some(&preview));
        update.details = true;
        update.grade_page = page;
        view.update(update).unwrap();
        let (scene, hits) = compose(&view, 960, 720);
        let mut ids = hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>();
        ids.sort();
        assert_eq!(
            ids,
            if page == 0 {
                vec![66, 68]
            } else if page == 4 {
                vec![66, 67]
            } else {
                vec![66, 67, 68]
            }
        );
        assert_eq!(view.hit((755.0, 621.0)), Some(ControlId(66)));
        assert_eq!(view.hit((30.0, 180.0)), None);
        if page < 3 {
            label(&scene, "STORED HITS 9 MISSES 0");
            label(&scene, &format!("STORED GRADE {} COUNT 1", page * 4));
        } else if page == 3 {
            label(&scene, "SAVED REPLAY OPERATION PREFIX");
            label(&scene, "original-own.bkr");
        } else {
            label(&scene, "PEER-REPORTED NOT FINAL RANKING");
        }
        assert!(Arc::ptr_eq(
            preview.historical_score.as_ref().unwrap(),
            &score
        ));
        assert!(Arc::ptr_eq(
            preview.historical_comparison.as_ref().unwrap(),
            &comparison
        ));
        {
            let cache = view.detail_cache.borrow();
            let cache = cache.as_ref().unwrap();
            assert!(Arc::ptr_eq(cache.score.as_ref().unwrap(), &score));
            assert!(Arc::ptr_eq(cache.comparison.as_ref().unwrap(), &comparison));
            assert_eq!(cache.presentation.comparison(), Some(comparison.as_ref()));
            let pointer = cache
                .presentation
                .comparison()
                .unwrap()
                .as_ref()
                .unwrap()
                .ghosts
                .as_ptr();
            assert_eq!(pointer, *presenter_pointer.get_or_insert(pointer));
        }
        let mut unchanged = frame(&directory, &catalog, Some(&preview));
        unchanged.details = true;
        unchanged.grade_page = page;
        view.update(unchanged).unwrap();
        assert!(!view.dirty());
        assert_eq!(
            view.detail_cache
                .borrow()
                .as_ref()
                .unwrap()
                .presentation
                .comparison()
                .unwrap()
                .as_ref()
                .unwrap()
                .ghosts
                .as_ptr(),
            presenter_pointer.unwrap()
        );
        assert!(scene.playfields().is_empty());
    }
    view.update(frame(&directory, &catalog, Some(&preview)))
        .unwrap();
    let (_, hits) = compose(&view, 960, 720);
    assert!(hits.iter().any(|(id, _)| *id == ControlId(50000)));
    assert_eq!(view.hit((755.0, 621.0)), Some(ControlId(55)));
}

#[test]
fn row_size_reflows_actual_following_rows_without_remount_and_clip_controls_paint_and_hits() {
    let catalog = catalog(10);
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(1203), 960, 720).unwrap();
    view.update(frame(&directory, &catalog, None)).unwrap();
    compose(&view, 960, 720);
    let identities = view.nodes.identities();
    let row = row_node(&view, 0);
    assert!(view
        .update_layout(&[LayoutUpdate {
            id: row,
            change: LayoutChange::Size([906, 20])
        }])
        .unwrap());
    let (scene, hits) = compose(&view, 960, 720);
    assert_eq!(view.nodes.identities(), identities);
    assert_eq!(
        hits.iter().find(|(id, _)| id.0 == 50000).unwrap().1.height,
        20
    );
    assert_eq!(hits.iter().find(|(id, _)| id.0 == 50001).unwrap().1.y, 192);
    assert_eq!(view.hit((30.0, 191.0)), None);
    assert_eq!(view.hit((30.0, 195.0)), Some(ControlId(50001)));
    assert!(scene
        .rectangles()
        .iter()
        .any(|r| r.bounds == [24.0, 192.0, 906.0, 28.0]));
    assert!(view
        .update_layout(&[LayoutUpdate {
            id: row,
            change: LayoutChange::Clip(Some(Bounds {
                x: 0,
                y: 0,
                width: 200,
                height: 10
            }))
        }])
        .unwrap());
    let (scene, hits) = compose(&view, 960, 720);
    let clipped = hits.iter().find(|(id, _)| id.0 == 50000).unwrap().1;
    assert_eq!(
        (clipped.x, clipped.y, clipped.width, clipped.height),
        (24, 170, 200, 10)
    );
    assert_eq!(view.hit((30.0, 175.0)), Some(ControlId(50000)));
    assert_eq!(view.hit((30.0, 185.0)), None);
    assert!(scene
        .rectangles()
        .iter()
        .any(|r| r.bounds == [24.0, 170.0, 200.0, 10.0]));
}

#[test]
fn actual_extent_clips_controls_zero_suspends_and_invalid_updates_leave_packet_unchanged() {
    let catalog = catalog(10);
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(1204), 960, 720).unwrap();
    view.update(frame(&directory, &catalog, None)).unwrap();
    let identities = view.nodes.identities();
    assert!(view.resize(480, 360).unwrap());
    let (scene, hits) = compose(&view, 480, 360);
    assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 480.0, 360.0]);
    assert!(scene.rectangles().iter().all(|r| r.bounds[0] >= 0.0
        && r.bounds[1] >= 0.0
        && r.bounds[0] + r.bounds[2] <= 480.0
        && r.bounds[1] + r.bounds[3] <= 360.0));
    assert_eq!(
        hits.iter().find(|(id, _)| id.0 == 50000).unwrap().1.width,
        456
    );
    assert_eq!(view.hit((479.0, 175.0)), Some(ControlId(50000)));
    assert_eq!(view.hit((480.0, 175.0)), None);
    assert_eq!(view.hit((25.0, 625.0)), None);
    let paints = view.nodes.paints();
    assert!(!view.resize(480, 360).unwrap());
    assert_eq!(view.nodes.paints(), paints);
    let before = packet(&scene);
    assert!(view
        .update_layout(&[
            LayoutUpdate {
                id: row_node(&view, 0),
                change: LayoutChange::Size([906, 20])
            },
            LayoutUpdate {
                id: NodeId(usize::MAX),
                change: LayoutChange::Size([1, 1])
            },
        ])
        .is_err());
    assert_eq!(packet(&compose(&view, 480, 360).0), before);
    assert_eq!(view.nodes.paints(), paints);
    assert!(view.resize(0, 360).unwrap());
    let (scene, hits) = compose(&view, 0, 360);
    assert!(scene.rectangles().is_empty());
    assert!(hits.is_empty());
    assert_eq!(view.hit((30.0, 175.0)), None);
    assert!(view.resize(960, 720).unwrap());
    compose(&view, 960, 720);
    assert_eq!(view.nodes.identities(), identities);
    assert_eq!(view.hit((30.0, 195.0)), Some(ControlId(50000)));
}

#[test]
fn pending_errors_invalid_frames_and_disposal_preserve_records_lifecycle() {
    let catalog = catalog(1);
    let directory = LineEditor::new("records", 4096).unwrap();
    let mut preview = preview(&catalog.entries[0]);
    let view = RecordsView::new(ScreenInstanceId(1205), 960, 720).unwrap();
    view.update(frame(&directory, &catalog, Some(&preview)))
        .unwrap();
    let mut pending = frame(&directory, &catalog, Some(&preview));
    pending.pending = true;
    pending.hovered = Some(ControlId(55));
    pending.armed = Some(ControlId(55));
    view.update(pending).unwrap();
    let (scene, hits) = compose(&view, 960, 720);
    label(&scene, "LOADING RECORDS");
    assert!(hits.is_empty());
    assert_eq!(view.hit((755.0, 621.0)), None);
    preview.historical = None;
    preview.historical_score = None;
    preview.historical_comparison = None;
    preview.archive_error = Some("private invalid archive row".into());
    let mut error = frame(&directory, &catalog, Some(&preview));
    error.error = Some("SCAN FAILED");
    view.update(error).unwrap();
    let (scene, _) = compose(&view, 960, 720);
    label(&scene, "HISTORICAL ARCHIVE UNAVAILABLE");
    label(&scene, "SCAN FAILED");
    let before = packet(&scene);
    let mut invalid = frame(&directory, &catalog, Some(&preview));
    invalid.selected = Some(1);
    assert!(view.update(invalid).is_err());
    let mut invalid = frame(&directory, &catalog, Some(&preview));
    invalid.details = true;
    assert!(view.update(invalid).is_err());
    assert_eq!(packet(&compose(&view, 960, 720).0), before);
    assert_eq!(view.hit((755.0, 621.0)), Some(ControlId(55)));
    let weak = view.nodes.weak_dirty();
    drop(view);
    assert!(weak.upgrade().is_none());
}

#[test]
fn mounted_directory_keeps_prepared_unicode_font_and_only_its_dependent_packet_changes() {
    use crate::{font_atlas::FontAtlas, texture::TextureId};
    let mut atlas = FontAtlas::new(crate::font_fixture::font_bytes(), 14.0, 128, 128, 16).unwrap();
    atlas.prepare('A').unwrap();
    atlas.prepare('가').unwrap();
    let texture = TextureId::allocate().unwrap();
    let font = FontText::new(Arc::new(atlas), texture).unwrap();
    let catalog = catalog(1);
    let directory = LineEditor::new("A가", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(1206), 960, 720).unwrap();
    view.update(frame(&directory, &catalog, None)).unwrap();
    compose(&view, 960, 720);
    let before = view.nodes.paints();
    view.set_input_font(Some(font.clone()));
    let (scene, _) = compose(&view, 960, 720);
    assert!(scene.batches().iter().any(|batch| batch.texture == texture));
    assert_eq!(
        view.nodes
            .paints()
            .iter()
            .zip(&before)
            .filter(|(a, b)| a != b)
            .count(),
        1
    );
    let paints = view.nodes.paints();
    view.set_input_font(Some(font));
    assert_eq!(view.nodes.paints(), paints);
    assert_eq!(view.hit((170.0, 115.0)), Some(ControlId(58)));
    assert_eq!(packet(&compose(&view, 960, 720).0), packet(&scene));
}

#[test]
fn cached_grade_and_comparison_details_suspend_update_and_restore_original_data_and_actions() {
    let catalog = catalog(1);
    let directory = LineEditor::new("records", 4096).unwrap();
    let preview = preview(&catalog.entries[0]);
    let score = preview.historical_score.as_ref().unwrap().clone();
    let comparison = preview.historical_comparison.as_ref().unwrap().clone();
    for page in [1, 3, 4] {
        let view = RecordsView::new(ScreenInstanceId(1210 + page as u64), 960, 720).unwrap();
        let detail_frame = || {
            let mut update = frame(&directory, &catalog, Some(&preview));
            update.details = true;
            update.grade_page = page;
            update
        };
        view.update(detail_frame()).unwrap();
        let presenter_pointer = view
            .detail_cache
            .borrow()
            .as_ref()
            .unwrap()
            .presentation
            .comparison()
            .unwrap()
            .as_ref()
            .unwrap()
            .ghosts
            .as_ptr();
        let (original, original_hits) = compose(&view, 960, 720);
        let expected_packet = packet(&original);
        let expected_hits = original_hits
            .iter()
            .map(|(id, b)| (id.0, [b.x, b.y, b.width, b.height]))
            .collect::<Vec<_>>();
        let identities = view.nodes.identities();

        assert!(view.resize(0, 0).unwrap());
        for _ in 0..2 {
            let (empty, hits) = compose(&view, 0, 0);
            assert!(empty.rectangles().is_empty());
            assert!(hits.is_empty());
            assert_eq!(view.hit((755.0, 621.0)), None);
            // Updating an already cached historical page is valid while the
            // viewport is suspended and must retain its original metadata.
            view.update(detail_frame()).unwrap();
            let cache = view.detail_cache.borrow();
            let cache = cache.as_ref().unwrap();
            assert!(Arc::ptr_eq(cache.score.as_ref().unwrap(), &score));
            assert!(Arc::ptr_eq(cache.comparison.as_ref().unwrap(), &comparison));
            assert_eq!(cache.presentation.comparison(), Some(comparison.as_ref()));
            assert_eq!(
                cache
                    .presentation
                    .comparison()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .ghosts
                    .as_ptr(),
                presenter_pointer
            );
        }
        assert!(view.resize(960, 720).unwrap());
        let (restored, restored_hits) = compose(&view, 960, 720);
        {
            let cache = view.detail_cache.borrow();
            let cache = cache.as_ref().unwrap();
            assert!(Arc::ptr_eq(cache.score.as_ref().unwrap(), &score));
            assert!(Arc::ptr_eq(cache.comparison.as_ref().unwrap(), &comparison));
            assert_eq!(cache.presentation.comparison(), Some(comparison.as_ref()));
            assert_eq!(
                cache
                    .presentation
                    .comparison()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .ghosts
                    .as_ptr(),
                presenter_pointer
            );
        }
        assert_eq!(packet(&restored), expected_packet);
        assert_eq!(
            restored_hits
                .iter()
                .map(|(id, b)| (id.0, [b.x, b.y, b.width, b.height]))
                .collect::<Vec<_>>(),
            expected_hits
        );
        assert_eq!(view.nodes.identities(), identities);
        assert_eq!(view.hit((755.0, 621.0)), Some(ControlId(66)));
        assert_eq!(view.hit((431.0, 576.0)), Some(ControlId(67)));
        assert_eq!(
            view.hit((581.0, 576.0)),
            (page < 4).then_some(ControlId(68))
        );
        assert_eq!(view.hit((30.0, 180.0)), None);
        if page == 1 {
            label(&restored, "STORED HITS 9 MISSES 0");
            label(&restored, "STORED GRADE 4 COUNT 1");
        } else if page == 3 {
            label(&restored, "SAVED REPLAY OPERATION PREFIX");
            label(&restored, "original-own.bkr");
        } else {
            label(&restored, "PEER-REPORTED NOT FINAL RANKING");
        }
    }
}
