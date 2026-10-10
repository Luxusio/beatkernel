//! Actual Desktop catalog polling, retirement and cross-worker cache propagation.
use super::*;
use beatkernel::{
    audio::{AudioFormat, PcmLimits, SampleId},
    time::{ClockDomainId, Timestamp},
};
use beatkernel_bms_runtime::{
    native_chart::{prepare_retained_chart_with_policy, NativeChartConfig},
    native_judge::NativeJudgeConfig,
    play_policy::GaugeSelection,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Mutex, OnceLock,
};
const WAIT: Duration = Duration::from_secs(5);
struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "beatkernel-desktop-loading-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(
            path.join("z.bms"),
            "#TITLE KEEP ZULU\n#BPM 120\n#WAV01 key.wav\n#00011:01\n",
        )
        .unwrap();
        std::fs::write(path.join("a.bms"), "#TITLE KEEP ALPHA\n#BPM 120\n").unwrap();
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
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
        std::fs::write(path.join("key.wav"), wav).unwrap();
        Self(path)
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn poll_until(app: &mut Desktop, condition: impl Fn(&Desktop) -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition(app) {
        app.collect_catalog();
        assert!(
            Instant::now() < deadline,
            "actual catalog condition did not complete"
        );
        thread::yield_now();
    }
}
fn selected_path(app: &Desktop) -> Option<PathBuf> {
    app.catalog_search
        .selected()
        .map(|index| app.entries[index].path.clone())
}
fn clear_selection(app: &mut Desktop) {
    app.entries.clear();
    app.selection_items = Arc::from([]);
    app.catalog_search = CatalogSearch::new(&[]).unwrap();
    app.selection_view = None;
    app.painted_reactive = None;
}

fn scene_text_footprints(
    scene: &Scene,
    value: &str,
    scale: usize,
    color: u32,
) -> Option<Vec<[f32; 4]>> {
    let mut expected = Scene::new(WIDTH as u32, HEIGHT as u32);
    text(&mut expected, 0, 0, value, scale, color);
    let glyphs = expected.rectangles();
    scene.rectangles().windows(glyphs.len()).find_map(|run| {
        let origin = run.first()?.bounds;
        run.iter()
            .zip(glyphs)
            .all(|(actual, expected)| {
                actual.uv == expected.uv
                    && actual.color == expected.color
                    && actual.bounds[2..] == expected.bounds[2..]
                    && actual.bounds[0] - origin[0] == expected.bounds[0]
                    && actual.bounds[1] == origin[1]
            })
            .then(|| run.iter().map(|glyph| glyph.bounds).collect())
    })
}

fn footprints_overlap(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

#[test]
fn desktop_actual_loading_progress_clears_status_practice_and_cancel_then_hides_when_playing() {
    let mut app = tests::lifecycle_fixture();
    let route = app
        .prepare_route(ScreenRoute::Play { replay: false })
        .unwrap();
    app.commit_route(route);
    let (publisher, viewer) = player::channel();
    app.game = Some(Game {
        viewer,
        worker: None,
        snapshot: None,
        completed_results: None,
        completed_results_error: None,
        cancelling: false,
        joined: false,
        local_page: 0,
        local_comparisons: false,
        replay: false,
        launch: SessionLaunch::new(vec!["--chart".into(), "loading.bms".into()]).unwrap(),
        prepared_retry: None,
        practice_bookmark: None,
        practice_loop: None,
        loop_enabled: false,
    });
    player::with_publisher(publisher, || {
        let context = player::preparation_context().unwrap();
        let progress = beatkernel_bms_runtime::preparation::PreparationProgress {
            stage: beatkernel_bms_runtime::preparation::PreparationStage::Read,
            completed_assets: 3,
            total_assets: 17,
        };
        let label = "PREPARING Read  3/17 RESOURCES - ESC CANCEL";
        for with_snapshot in [false, true] {
            if with_snapshot {
                let source = beatkernel_bms::parse(
                    "#TITLE Loading\n#BPM 120\n",
                    beatkernel_bms::ParseOptions::default(),
                )
                .unwrap();
                let chart = source.compile().unwrap().chart;
                player::publish_chart(&source, &chart).unwrap();
                let game = app.game.as_mut().unwrap();
                let snapshot = game.viewer.take_latest().unwrap();
                assert_eq!(snapshot.status, player::PlayerStatus::Loading);
                game.accept_snapshot(snapshot);
            }
            context.check(progress).unwrap();
            assert_eq!(
                app.game.as_ref().unwrap().viewer.preparation_progress(),
                Some(progress)
            );
            app.draw().unwrap();
            let preparation = scene_text_footprints(&app.scene, label, 1, 0x9bb1cf)
                .expect("actual preparation stage, resource counts and ESC Cancel must draw");
            let status = scene_text_footprints(
                &app.scene,
                if with_snapshot {
                    "LOADING"
                } else {
                    "LOADING - ESC CANCEL"
                },
                2,
                0x9bb1cf,
            )
            .expect("actual loading status must draw");
            let practice = scene_text_footprints(
                &app.scene,
                "F7 MARK / F8 RESTART MARK  FRESH OUTPUT; MAY GAP",
                1,
                0xd8b36b,
            )
            .expect("actual practice instructions must remain readable");
            for glyph in &preparation {
                for other in status.iter().chain(&practice) {
                    assert!(
                        !footprints_overlap(*glyph, *other),
                        "preparation glyph {glyph:?} overlaps status/practice glyph {other:?}"
                    );
                }
                for (_, bounds) in &app.hits {
                    assert!(
                        !footprints_overlap(
                            *glyph,
                            [
                                bounds.x as f32,
                                bounds.y as f32,
                                bounds.width as f32,
                                bounds.height as f32,
                            ]
                        ),
                        "preparation glyph overlaps an actual control"
                    );
                }
            }
            assert!(app.hits.iter().any(|(id, _)| *id == ControlId(2)));
            assert!(scene_text_footprints(&app.scene, "CANCEL", 2, 0xf0f4ff).is_some());
        }
        player::publish_replay_prefix(Timestamp::ZERO, &[]).unwrap();
        player::publish_pause(player::PauseState::Running);
        let game = app.game.as_mut().unwrap();
        let snapshot = game.viewer.take_latest().unwrap();
        assert_eq!(snapshot.status, player::PlayerStatus::Playing);
        game.accept_snapshot(snapshot);
        assert_eq!(game.viewer.preparation_progress(), Some(progress));
        app.draw().unwrap();
        assert!(scene_text_footprints(&app.scene, label, 1, 0x9bb1cf).is_none());
        assert!(scene_text_footprints(&app.scene, "PLAYING - ESC CANCEL", 2, 0x9bb1cf).is_some());
        assert!(app.hits.iter().any(|(id, _)| *id == ControlId(2)));
        Ok(())
    })
    .unwrap();
}

fn completion_after_pending_poll_preserves_catalog_result(failure: Option<&'static str>) {
    let tree = Tree::new();
    let font = tree.0.join("completion.ttf");
    std::fs::write(&font, font_fixture::font_bytes()).unwrap();
    let mut app = tests::lifecycle_fixture();
    app.options.library = Some(tree.0.clone());
    clear_selection(&mut app);
    app.catalog_generation = 85;
    app.catalog_screen = app.navigator.active_id();
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    app.catalog = Some(
        NativeCatalog::spawn_streamed(tree.0.clone(), 85, move |library, control| {
            entered.send(()).unwrap();
            gate.recv_timeout(WAIT).map_err(|error| error.to_string())?;
            if let Some(error) = failure {
                return Err(error.into());
            }
            prepare_catalog(library, Some(font), Vec::new(), control)
        })
        .unwrap(),
    );
    poll_until(&mut app, |app| app.catalog_metadata_complete);
    ready.recv_timeout(WAIT).unwrap();
    assert_eq!(app.entries.len(), 2);
    assert!(app.title_font.is_none());
    assert!(app.pending_catalog.is_none());
    assert!(app.catalog.as_mut().unwrap().poll().is_none());
    assert!(!app.catalog.as_ref().unwrap().result_consumed());

    // Force the reviewed interleaving: the actual poll was pending, then the
    // actual worker finishes before the production post-poll release decision.
    release.send(()).unwrap();
    let deadline = Instant::now() + WAIT;
    while !app.catalog.as_ref().unwrap().is_finished() {
        assert!(
            Instant::now() < deadline,
            "held catalog worker did not finish"
        );
        thread::yield_now();
    }
    assert!(!app.catalog.as_ref().unwrap().result_consumed());
    app.release_drained_catalog();
    assert!(
        app.catalog.is_some(),
        "metadata completion cannot release an unconsumed font/error result"
    );
    assert!(app.pending_catalog.is_none());
    assert!(app.title_font.is_none());

    // The next actual collect must consume and publish the retained result.
    app.collect_catalog();
    assert!(app.catalog.is_none());
    assert!(app.pending_catalog.is_none());
    assert_eq!(app.entries.len(), 2);
    if let Some(error) = failure {
        assert_eq!(
            app.catalog_message,
            Some(format!("Catalog unavailable: {error}"))
        );
        assert!(app.title_font.is_none());
    } else {
        assert!(
            app.title_font.is_some(),
            "the real prepared font must publish"
        );
        assert!(app.catalog_message.is_none());
    }
    let message = app.catalog_message.clone();
    app.collect_catalog();
    assert_eq!(app.catalog_message, message);
    assert_eq!(app.title_font.is_some(), failure.is_none());
    assert!(app.game.is_none());
}

#[test]
fn desktop_finished_after_pending_poll_retains_owner_until_real_font_publication() {
    completion_after_pending_poll_preserves_catalog_result(None);
}

#[test]
fn desktop_finished_after_pending_poll_retains_owner_until_explicit_error_publication() {
    completion_after_pending_poll_preserves_catalog_result(Some("held title font rejected"));
}

#[test]
fn desktop_real_partial_metadata_is_selectable_while_font_owner_is_held() {
    let tree = Tree::new();
    let font = tree.0.join("fixture.ttf");
    std::fs::write(&font, font_fixture::font_bytes()).unwrap();
    let mut app = tests::lifecycle_fixture();
    app.options.library = Some(tree.0.clone());
    clear_selection(&mut app);
    app.catalog_generation = 81;
    app.catalog_screen = app.navigator.active_id();
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    app.catalog = Some(
        NativeCatalog::spawn_streamed(tree.0.clone(), 81, move |library, control| {
            entered.send(()).unwrap();
            gate.recv_timeout(WAIT).map_err(|error| error.to_string())?;
            prepare_catalog(library, Some(font), Vec::new(), control)
        })
        .unwrap(),
    );
    poll_until(&mut app, |app| app.catalog_metadata_complete);
    ready.recv_timeout(WAIT).unwrap();
    assert_eq!(app.entries.len(), 2);
    assert!(!app.catalog.as_ref().unwrap().is_finished());
    assert!(app.title_font.is_none());
    app.draw().unwrap();
    assert!(app.hits.iter().any(|(id, _)| id.0 >= 100));
    app.set_search_focus(true);
    app.edit_search(None, Some("KEEP"));
    let keep = tree.0.join("z.bms");
    let index = app
        .entries
        .iter()
        .position(|entry| entry.path == keep)
        .unwrap();
    app.catalog_search.select(index).unwrap();
    app.selected = index;
    assert_eq!(selected_path(&app), Some(keep.clone()));
    assert_eq!(app.search_editor.value(), "KEEP");
    assert!(app.search_focused);
    app.open_settings();
    assert_eq!(app.navigator.route(), ScreenRoute::Settings);
    assert!(
        !app.search_focused,
        "navigation deliberately blurs the retained search editor"
    );
    let settings = app.settings.as_ref().unwrap().editor.clone();
    release.send(()).unwrap();
    let deadline = Instant::now() + WAIT;
    while !app.catalog.as_ref().is_none_or(NativeCatalog::is_finished) {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    app.collect_catalog();
    assert!(
        app.title_font.is_none(),
        "hidden Selection must not publish font into Settings"
    );
    assert_eq!(app.settings.as_ref().unwrap().editor, settings);
    app.back();
    assert!(
        !app.search_focused,
        "Back does not restore input focus automatically"
    );
    assert!(app.title_font.is_none());
    app.set_search_focus(true);
    poll_until(&mut app, |app| {
        app.catalog.is_none() && app.pending_catalog.is_none()
    });
    assert!(app.title_font.is_some());
    assert_eq!(selected_path(&app), Some(keep));
    assert_eq!(app.search_editor.value(), "KEEP");
    assert!(app.search_focused);
}

#[test]
fn desktop_batches_preserve_path_query_focus_across_resort_and_refuse_stale_generation() {
    let mut app = tests::lifecycle_fixture();
    app.catalog_generation = 82;
    app.catalog_screen = app.navigator.active_id();
    app.set_search_focus(true);
    app.edit_search(None, Some("FIX"));
    let selected = selected_path(&app).unwrap();
    app.install_catalog_entries(
        82,
        vec![player_chart::LibraryEntry {
            path: PathBuf::from("new.bms"),
            title: "AAA FIX".into(),
            artist: String::new(),
        }],
    )
    .unwrap();
    assert_eq!(selected_path(&app), Some(selected.clone()));
    assert_eq!(app.search_editor.value(), "FIX");
    assert!(app.search_focused);
    app.install_catalog_entries(
        82,
        vec![player_chart::LibraryEntry {
            path: selected.clone(),
            title: "ZZZ FIX RENAMED".into(),
            artist: "updated".into(),
        }],
    )
    .unwrap();
    assert_eq!(selected_path(&app), Some(selected));
    assert_eq!(
        app.entries.len(),
        2,
        "metadata update must not duplicate a canonical path"
    );
    let before = app
        .entries
        .iter()
        .map(|entry| (entry.path.clone(), entry.title.clone()))
        .collect::<Vec<_>>();
    assert!(app
        .install_catalog_entries(
            81,
            vec![player_chart::LibraryEntry {
                path: "stale.bms".into(),
                title: "STALE FIX".into(),
                artist: String::new()
            }]
        )
        .is_err());
    assert_eq!(
        before,
        app.entries
            .iter()
            .map(|entry| (entry.path.clone(), entry.title.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(app.search_editor.value(), "FIX");
    assert!(app.search_focused);
}

#[test]
fn desktop_empty_catalog_and_scan_error_remain_nonplayable_and_reload_recovers() {
    let tree = Tree::new();
    let empty = tree.0.join("empty");
    std::fs::create_dir(&empty).unwrap();
    let mut app = tests::lifecycle_fixture();
    app.options.library = Some(empty);
    app.begin_selection().unwrap();
    poll_until(&mut app, |app| {
        app.catalog.is_none() && app.pending_catalog.is_none()
    });
    assert!(app.entries.is_empty());
    assert!(app.catalog_search.selected().is_none());
    assert!(app.start().is_err());
    app.draw().unwrap();
    assert!(!app.hits.iter().any(|(control, _)| control.0 >= 100));
    app.options.library = Some(tree.0.join("missing"));
    app.begin_selection().unwrap();
    poll_until(&mut app, |app| {
        app.catalog.is_none() && app.pending_catalog.is_none()
    });
    assert!(app.entries.is_empty());
    assert!(app
        .catalog_message
        .as_ref()
        .is_some_and(|message| message.contains("unavailable") || message.contains("error")));
    assert!(app.start().is_err());
    app.options.library = Some(tree.0.clone());
    app.begin_selection().unwrap();
    poll_until(&mut app, |app| {
        app.catalog.is_none() && app.pending_catalog.is_none()
    });
    assert_eq!(app.entries.len(), 2);
    assert!(app.catalog_search.selected().is_some());
    app.draw().unwrap();
    assert!(app.hits.iter().any(|(control, _)| control.0 >= 100));
    assert!(
        app.game.is_none(),
        "catalog recovery never fabricates playable/output readiness"
    );
}

#[test]
fn desktop_one_actual_poll_drains_at_most_two_bounded_batches() {
    let tree = Tree::new();
    for index in 0..150 {
        std::fs::write(
            tree.0.join(format!("many-{index:03}.bms")),
            format!("#TITLE MANY {index:03}\n#BPM 120\n"),
        )
        .unwrap();
    }
    let mut app = tests::lifecycle_fixture();
    clear_selection(&mut app);
    app.options.library = Some(tree.0.clone());
    app.catalog_generation = 84;
    app.catalog_screen = app.navigator.active_id();
    app.catalog = Some(
        NativeCatalog::spawn_streamed(tree.0.clone(), 84, |library, control| {
            prepare_catalog(library, None, Vec::new(), control)
        })
        .unwrap(),
    );
    let deadline = Instant::now() + WAIT;
    while app.catalog.as_mut().unwrap().progress().charts < 129 {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(app.entries.is_empty());
    app.collect_catalog();
    assert_eq!(
        app.entries.len(),
        65,
        "first entry plus one 64-entry batch are the two available messages"
    );
    assert!(!app.catalog_metadata_complete);
    assert!(app.catalog.as_ref().is_some_and(|job| !job.is_finished()));
    app.retire_catalog().unwrap();
    while !app.retired_catalogs.is_empty() {
        app.collect_retired_catalogs();
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert_eq!(
        app.entries.len(),
        65,
        "retired owner's remaining messages never publish later"
    );
}

fn held_catalog() -> (NativeCatalog<PreparedCatalog>, mpsc::SyncSender<()>) {
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let job = NativeCatalog::spawn_prepared(move |_| {
        entered.send(()).unwrap();
        gate.recv_timeout(WAIT).map_err(|error| error.to_string())?;
        Ok(PreparedCatalog {
            entries: Vec::new(),
            items: Arc::from([]),
            search: CatalogSearch::new(&[]).unwrap(),
            diagnostics: Arc::from([]),
            font: None,
        })
    })
    .unwrap();
    ready.recv_timeout(WAIT).unwrap();
    (job, release)
}

#[test]
fn desktop_retirement_is_bounded_and_does_not_join_unfinished_jobs_during_ui_events() {
    let mut app = tests::lifecycle_fixture();
    let mut release = Vec::new();
    for expected in 1..=2 {
        let (job, gate) = held_catalog();
        app.catalog = Some(job);
        app.retire_catalog().unwrap();
        release.push(gate);
        assert!(app.catalog.is_none());
        assert_eq!(app.retired_catalogs.len(), expected);
        assert!(app.retired_catalogs.iter().all(|job| !job.is_finished()));
    }
    let (active, active_gate) = held_catalog();
    app.catalog = Some(active);
    assert!(app.retire_catalog().is_err());
    assert!(app.catalog.is_some());
    assert_eq!(app.retired_catalogs.len(), 2);
    app.collect_retired_catalogs();
    assert_eq!(app.retired_catalogs.len(), 2);
    app.set_search_focus(true);
    app.edit_search(None, Some("FIX"));
    assert_eq!(app.search_editor.value(), "FIX");
    assert!(app.game.is_none());
    for gate in release {
        gate.send(()).unwrap();
    }
    let deadline = Instant::now() + WAIT;
    while !app.retired_catalogs.is_empty() {
        app.collect_retired_catalogs();
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(!app.catalog.as_ref().unwrap().is_finished());
    active_gate.send(()).unwrap();
    while !app.catalog.as_ref().unwrap().is_finished() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    app.retire_catalog().unwrap();
    app.collect_retired_catalogs();
    assert!(app.retired_catalogs.is_empty());
}

type PreparedReply = (beatkernel_bms_runtime::PreparedBms, thread::ThreadId, usize);
static PREPARED: OnceLock<Mutex<Option<mpsc::SyncSender<PreparedReply>>>> = OnceLock::new();
fn injected_native_prepare(args: &[String]) -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(
        args.chunks_exact(2)
            .find(|pair| pair[0] == "--chart")
            .unwrap()[1]
            .clone(),
    );
    let bindings = std::collections::BTreeMap::from([(0x11, 4)]);
    let original = prepare_retained_chart_with_policy(
        NativeChartConfig {
            path: &path,
            format: AudioFormat::new(24_000, 1).unwrap(),
            limits: PcmLimits::new(1024, 4096, 8).unwrap(),
            channels: beatkernel_bms_runtime::ChannelPolicy::Exact,
            chart_seed: 0,
            start: Timestamp::ZERO,
            bindings: &bindings,
        },
        &NativeJudgeConfig {
            early: 0,
            late: 0,
            offset: 0,
            preroll: 0,
            output: ClockDomainId(9),
            end: None,
        },
        GaugeSelection::BeatKernel,
        None,
    )?
    .original;
    let cache = player::preparation_context().unwrap().cache().unwrap();
    PREPARED
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .send((
            original,
            thread::current().id(),
            Arc::as_ptr(&cache) as usize,
        ))
        .unwrap();
    Ok(())
}
fn join_game(app: &mut Desktop) {
    let deadline = Instant::now() + WAIT;
    while !app.game.as_ref().unwrap().joined {
        app.collect_game();
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
}

#[test]
fn desktop_actual_initial_spawn_and_joined_retry_share_cache_across_different_workers() {
    let tree = Tree::new();
    let (sender, receiver) = mpsc::sync_channel(2);
    *PREPARED.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(sender);
    let mut app = tests::lifecycle_fixture();
    app.options = Options::parse(&[
        "--chart".into(),
        tree.0.join("z.bms").to_str().unwrap().into(),
    ])
    .unwrap();
    app.native = injected_native_prepare;
    app.begin_selection().unwrap();
    assert!(
        app.catalog_generation > 0,
        "production first Selection initialization precedes playback"
    );
    let cache = Arc::clone(&app.preparation_cache);
    app.start().unwrap();
    let (cold, first_thread, first_owner) = receiver.recv_timeout(WAIT).unwrap();
    join_game(&mut app);
    let retry = app.game.as_ref().unwrap().launch.retry().unwrap();
    app.replace_joined_game(retry);
    let (warm, second_thread, second_owner) = receiver.recv_timeout(WAIT).unwrap();
    join_game(&mut app);
    assert_ne!(first_thread, second_thread);
    assert_eq!(first_owner, Arc::as_ptr(&cache) as usize);
    assert_eq!(second_owner, first_owner);
    assert_eq!(
        cold.bank.get(SampleId(1)).unwrap().samples().as_ptr(),
        warm.bank.get(SampleId(1)).unwrap().samples().as_ptr()
    );
    assert_eq!(app.game.as_ref().unwrap().launch.attempt(), 1);
    app.navigate(ScreenRoute::Selection).unwrap();
    app.options.library = Some(tree.0.clone());
    app.begin_selection().unwrap();
    assert!(!Arc::ptr_eq(&cache, &app.preparation_cache));
    assert_eq!(cold.bank.get(SampleId(1)).unwrap().samples(), [0.25]);
    *PREPARED.get().unwrap().lock().unwrap() = None;
}
