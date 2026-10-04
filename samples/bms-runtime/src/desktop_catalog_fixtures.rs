//! Deferred actual Desktop catalog ownership; no window, renderer or audio opens.
use super::*;
use std::{
    fs,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};

struct CatalogTree(PathBuf);
impl CatalogTree {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "beatkernel-desktop-catalog-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(
            path.join("alpha.bms"),
            "#TITLE Alpha\n#ARTIST First\n#BPM 60\n",
        )
        .unwrap();
        fs::write(
            path.join("etoile.bms"),
            "#TITLE ÉTOILE\n#ARTIST 作曲家\n#BPM 60\n#WAV01 absent.wav\n#00011:01\n",
        )
        .unwrap();
        fs::write(path.join("invalid.bms"), [0xff, 0xfe]).unwrap();
        fs::write(path.join("title.ttf"), font_fixture::font_bytes()).unwrap();
        fs::write(path.join("invalid.ttf"), [0u8; 32]).unwrap();
        Self(path)
    }
}
impl Drop for CatalogTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
const WAIT: Duration = Duration::from_secs(5);

fn empty_selection() -> Desktop {
    let mut app = super::tests::lifecycle_fixture();
    app.entries.clear();
    app.selection_items = Arc::from([]);
    app.catalog_search = CatalogSearch::new(&[]).unwrap();
    app.selection_diagnostics = Arc::from([]);
    app
}

fn gated_catalog(
    root: &CatalogTree,
    font: &str,
) -> (
    NativeCatalog<PreparedCatalog>,
    mpsc::Receiver<()>,
    mpsc::SyncSender<()>,
) {
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let font = root.0.join(font);
    let job = NativeCatalog::spawn(root.0.clone(), move |library, control| {
        assert_eq!(library.entries.len(), 2);
        assert_eq!(library.diagnostics.len(), 1);
        entered.send(()).unwrap();
        gate.recv_timeout(WAIT).map_err(|error| error.to_string())?;
        prepare_catalog(library, Some(font), control)
    })
    .unwrap();
    (job, ready, release)
}

fn wait_for_actual_completion(app: &Desktop) {
    // The gate determines progress. This bounded yield only observes the real
    // JoinHandle completion; it never creates a receipt or changes job state.
    let deadline = Instant::now() + WAIT;
    while !app.catalog.as_ref().unwrap().is_finished() {
        assert!(
            Instant::now() < deadline,
            "catalog worker did not complete its released operation"
        );
        thread::yield_now();
    }
}

#[test]
fn desktop_loading_blocks_launch_until_the_actual_catalog_search_diagnostics_and_font_install_together()
 {
    let root = CatalogTree::new();
    let (job, ready, release) = gated_catalog(&root, "title.ttf");
    let mut app = empty_selection();
    app.catalog = Some(job);
    ready.recv_timeout(WAIT).unwrap();
    app.collect_catalog();
    assert_eq!(
        app.catalog_progress.stage,
        player_chart::ScanStage::Complete
    );
    assert_eq!(app.catalog_progress.charts, 3);
    assert!(
        app.catalog_message
            .as_deref()
            .unwrap()
            .contains("Preparing")
    );
    assert!(app.entries.is_empty());
    assert!(app.selection_items.is_empty());
    assert!(app.catalog_search.selected().is_none());
    assert!(app.title_font.is_none());
    assert!(app.selection_diagnostics.is_empty());
    assert!(app.start().is_err());
    assert!(app.game.is_none());
    assert!(
        !app.reactive_waits_for_events(),
        "background ownership must still be polled without a UI input event"
    );
    app.draw_selection().unwrap();
    app.scene.status().unwrap();
    assert_eq!(
        app.hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
        vec![5, 4],
        "loading exposes Settings/Exit, with no Start or partial chart hit"
    );
    app.set_search_focus(true);
    assert!(!app.search_focused);
    let screen = app.navigator.active_id();
    for _ in 0..8 {
        app.collect_catalog();
        assert!(app.entries.is_empty());
    }
    release.send(()).unwrap();
    wait_for_actual_completion(&app);
    assert!(
        app.start().is_err(),
        "thread completion alone has not atomically installed selection state"
    );
    app.collect_catalog();
    assert!(app.catalog.is_none());
    assert!(app.catalog_message.is_none());
    assert_eq!(
        app.entries
            .iter()
            .map(|entry| entry.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Alpha", "ÉTOILE"]
    );
    assert_eq!(app.entries[1].path, root.0.join("etoile.bms"));
    assert_eq!(app.selection_items[1].artist, "作曲家");
    assert_eq!(app.selection_diagnostics.len(), 1);
    assert!(app.selection_diagnostics[0].contains("invalid.bms"));
    assert_eq!(app.catalog_search.indices().as_ref(), &[0, 1]);
    assert_eq!(app.catalog_search.selected(), Some(0));
    assert_eq!(app.search_editor.value(), "");
    assert_eq!(app.navigator.active_id(), screen);
    assert!(app.selection_view.is_none());
    assert!(app.title_font.as_ref().unwrap().get('É').is_some());
    assert!(app.title_font.as_ref().unwrap().get('作').is_some());
    assert!(
        app.font_text.is_none(),
        "no renderer was constructed; CPU font preparation cannot upload a texture"
    );
    let items = app.selection_items.clone();
    let font = app.title_font.as_ref().unwrap().clone();
    app.catalog_search.set_query("étoile 作曲家").unwrap();
    assert_eq!(app.catalog_search.indices().as_ref(), &[1]);
    for _ in 0..8 {
        app.collect_catalog();
    }
    assert!(Arc::ptr_eq(&items, &app.selection_items));
    assert!(Arc::ptr_eq(&font, app.title_font.as_ref().unwrap()));
    assert_eq!(app.catalog_search.indices().as_ref(), &[1]);
    app.draw_selection().unwrap();
    app.scene.status().unwrap();
    assert!(app.hits.iter().any(|(id, _)| id.0 == 101));
    assert!(app.game.is_none());
}

#[test]
fn hidden_suspended_closing_and_failed_catalogs_never_install_into_another_screen_or_publish_partial_selection()
 {
    for state in ["settings", "suspended", "occluded", "closing", "bad-font"] {
        let root = CatalogTree::new();
        let (job, ready, release) = gated_catalog(
            &root,
            if state == "bad-font" {
                "invalid.ttf"
            } else {
                "title.ttf"
            },
        );
        let mut app = empty_selection();
        app.catalog = Some(job);
        ready.recv_timeout(WAIT).unwrap();
        app.collect_catalog();
        app.draw_selection().unwrap();
        let original_screen = app.navigator.active_id();
        match state {
            "settings" => {
                app.open_settings();
                assert_eq!(app.navigator.route(), ScreenRoute::Settings);
                app.settings
                    .as_mut()
                    .unwrap()
                    .profile
                    .insert("retained-profile.bkp")
                    .unwrap();
            }
            "suspended" => app.navigator.suspend(),
            "occluded" => app.occluded = true,
            "closing" => app.request_close(),
            _ => {}
        }
        let settings_screen = app.settings.as_ref().map(|settings| settings.id());
        if state == "closing" {
            assert!(app.closing());
            assert!(
                !app.catalog.as_ref().unwrap().is_finished(),
                "close requests cancellation without blocking dispatch"
            );
            assert!(app.selection_view.is_none());
        }
        release.send(()).unwrap();
        wait_for_actual_completion(&app);
        app.collect_catalog();
        match state {
            "settings" | "suspended" => {
                assert!(app.catalog.as_ref().unwrap().is_finished());
                assert!(app.entries.is_empty());
                assert!(app.selection_items.is_empty());
                assert!(app.title_font.is_none());
                assert!(app.start().is_err());
                if state == "settings" {
                    assert_eq!(
                        app.settings.as_ref().unwrap().id(),
                        settings_screen.unwrap()
                    );
                    assert_eq!(
                        app.settings.as_ref().unwrap().profile.value(),
                        "retained-profile.bkp"
                    );
                    assert!(
                        app.selection_view.is_none(),
                        "hidden Selection is not reconstructed by background completion"
                    );
                    app.back();
                } else {
                    assert_eq!(app.navigator.active_id(), original_screen);
                    app.navigator.resume();
                }
                app.collect_catalog();
                assert!(app.catalog.is_none());
                assert_eq!(app.entries.len(), 2);
                assert!(app.title_font.is_some());
            }
            "occluded" => {
                assert!(app.catalog.is_none());
                assert_eq!(app.entries.len(), 2);
                assert!(
                    app.selection_view.is_none(),
                    "hidden completion installs data without creating a retained screen"
                );
                assert!(app.start().is_err());
                app.occluded = false;
                app.draw_selection().unwrap();
                app.scene.status().unwrap();
            }
            "closing" => {
                assert!(app.catalog.is_none());
                assert!(app.closing());
                assert!(app.entries.is_empty());
                assert!(app.selection_items.is_empty());
                assert!(app.title_font.is_none());
                assert!(app.selection_view.is_none());
                app.collect_catalog();
                assert!(app.entries.is_empty());
                assert!(app.start().is_err());
            }
            "bad-font" => {
                assert!(app.catalog.is_none());
                assert!(app.entries.is_empty());
                assert!(app.selection_items.is_empty());
                assert!(app.title_font.is_none());
                assert!(app.selection_diagnostics.is_empty());
                assert!(app.catalog_search.selected().is_none());
                assert!(
                    app.catalog_message
                        .as_deref()
                        .unwrap()
                        .contains("Catalog unavailable")
                );
                assert!(app.start().is_err());
                app.draw_selection().unwrap();
                app.scene.status().unwrap();
                assert!(app.hits.iter().all(|(id, _)| id.0 < 100));
            }
            _ => unreachable!(),
        }
        assert!(app.game.is_none());
        let replacement = super::tests::lifecycle_fixture();
        assert_eq!(replacement.entries[0].title, "FIXTURE");
        assert!(replacement.catalog.is_none());
        assert!(replacement.title_font.is_none());
    }
}

fn direct_options(path: &std::path::Path, font: Option<&std::path::Path>) -> Options {
    let mut args = vec!["--chart".into(), path.to_str().unwrap().into()];
    if let Some(font) = font {
        args.extend(["--title-font".into(), font.to_str().unwrap().into()]);
    }
    Options::parse(&args).unwrap()
}

fn gated_direct_catalog(
    path: PathBuf,
    font: PathBuf,
) -> (
    NativeCatalog<PreparedCatalog>,
    mpsc::Receiver<thread::ThreadId>,
    mpsc::SyncSender<()>,
) {
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let job = NativeCatalog::spawn_prepared(move |control| {
        entered.send(thread::current().id()).unwrap();
        gate.recv_timeout(WAIT).map_err(|error| error.to_string())?;
        prepare_direct_catalog(path, font, control)
    })
    .unwrap();
    (job, ready, release)
}

#[test]
fn direct_chart_route_prepares_literal_filename_search_and_font_without_reading_the_chart_or_parent()
 {
    let root = CatalogTree::new();
    // No such parent or chart is created. Even the literal '..' is retained:
    // direct title preparation must not canonicalize, scan or parse this input.
    let path = root
        .0
        .join("absent-parent")
        .join("..")
        .join("ÉTOILE 曲.bms");
    let cheap = direct_options(&path, None);
    assert!(spawn_catalog(&cheap).unwrap().is_none());
    let entry = direct_entry(path.clone());
    assert_eq!(entry.path, path);
    assert_eq!(entry.title, "ÉTOILE 曲.bms");
    assert_eq!(entry.artist, "");

    let options = direct_options(&path, Some(&root.0.join("title.ttf")));
    let job = spawn_catalog(&options).unwrap().unwrap();
    let mut app = empty_selection();
    app.options = options;
    app.catalog = Some(job);
    // A finished thread is still pending ownership until collect_catalog joins
    // it; these gates do not depend on the worker winning a scheduling race.
    assert!(app.entries.is_empty());
    assert!(app.selection_items.is_empty());
    assert!(app.catalog_search.selected().is_none());
    assert!(app.title_font.is_none());
    assert!(app.start().unwrap_err().contains("title font"));
    assert!(!app.reactive_waits_for_events());
    app.draw_selection().unwrap();
    app.scene.status().unwrap();
    assert_eq!(
        app.hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
        vec![5, 4]
    );
    app.set_search_focus(true);
    assert!(!app.search_focused);
    let screen = app.navigator.active_id();
    wait_for_actual_completion(&app);
    assert!(app.catalog.is_some());
    assert!(app.entries.is_empty());
    assert!(app.title_font.is_none());
    assert!(app.start().is_err());
    app.collect_catalog();
    assert!(app.catalog.is_none());
    assert!(app.catalog_message.is_none());
    assert_eq!(app.catalog_progress, player_chart::ScanProgress::default());
    assert_eq!(app.entries.len(), 1);
    assert_eq!(app.entries[0].path, path);
    assert_eq!(app.entries[0].title, "ÉTOILE 曲.bms");
    assert_eq!(app.entries[0].artist, "");
    assert_eq!(app.selection_items.len(), 1);
    assert_eq!(app.selection_items[0].title, "ÉTOILE 曲.bms");
    assert_eq!(app.selection_items[0].artist, "");
    assert!(app.selection_diagnostics.is_empty());
    assert_eq!(app.catalog_search.indices().as_ref(), &[0]);
    app.catalog_search.set_query("étoile 曲").unwrap();
    assert_eq!(app.catalog_search.indices().as_ref(), &[0]);
    app.catalog_search.set_query("First").unwrap();
    assert!(app.catalog_search.indices().is_empty());
    app.catalog_search.set_query("").unwrap();
    assert_eq!(app.catalog_search.selected(), Some(0));
    let font = app.title_font.as_ref().unwrap().clone();
    for character in ['É', 'T', 'O', 'I', 'L', 'E', ' ', '曲', '.', 'b', 'm', 's'] {
        assert!(font.get(character).is_some());
    }
    assert!(app.font_text.is_none());
    assert!(app.renderer.is_none());
    assert_eq!(app.navigator.active_id(), screen);
    let items = app.selection_items.clone();
    app.collect_catalog();
    assert!(Arc::ptr_eq(&items, &app.selection_items));
    assert!(Arc::ptr_eq(&font, app.title_font.as_ref().unwrap()));
    app.draw_selection().unwrap();
    app.scene.status().unwrap();
    assert!(app.hits.iter().any(|(id, _)| id.0 == 100));
    assert!(app.game.is_none());
}

#[test]
fn direct_font_failure_hidden_suspend_and_close_keep_prepared_data_owned_until_join_or_discard() {
    let root = CatalogTree::new();
    // Deferred filesystem setup: the bounded reader sees its one detection
    // byte and the shared atlas byte limit must refuse before publication.
    fs::File::create(root.0.join("oversized.ttf"))
        .unwrap()
        .set_len(32 * 1024 * 1024 + 1)
        .unwrap();
    for state in [
        "settings",
        "suspended",
        "closing",
        "missing",
        "invalid",
        "oversized",
    ] {
        let font_name = match state {
            "missing" => "missing.ttf",
            "invalid" => "invalid.ttf",
            "oversized" => "oversized.ttf",
            _ => "title.ttf",
        };
        let path = root.0.join("unread-parent").join("ÉTOILE 曲.bms");
        let font_path = root.0.join(font_name);
        let options = direct_options(&path, Some(&font_path));
        let (job, ready, release) = gated_direct_catalog(path.clone(), font_path);
        let mut app = empty_selection();
        app.options = options;
        app.catalog = Some(job);
        assert_ne!(ready.recv_timeout(WAIT).unwrap(), thread::current().id());
        app.collect_catalog();
        assert_eq!(app.catalog_progress, player_chart::ScanProgress::default());
        assert!(app.start().is_err());
        assert!(app.entries.is_empty());
        assert!(app.title_font.is_none());
        app.draw_selection().unwrap();
        app.scene.status().unwrap();
        assert_eq!(
            app.hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![5, 4]
        );
        match state {
            "settings" => {
                app.open_settings();
                app.settings
                    .as_mut()
                    .unwrap()
                    .profile
                    .insert("direct-profile.bkp")
                    .unwrap();
            }
            "suspended" => app.navigator.suspend(),
            "closing" => {
                app.request_close();
                assert!(app.closing());
                assert!(!app.catalog.as_ref().unwrap().is_finished());
            }
            _ => {}
        }
        let screen = app.navigator.active_id();
        release.send(()).unwrap();
        wait_for_actual_completion(&app);
        app.collect_catalog();
        assert!(app.entries.is_empty());
        assert!(app.selection_items.is_empty());
        assert!(app.selection_diagnostics.is_empty());
        assert!(app.title_font.is_none());
        assert!(app.font_text.is_none());
        assert!(app.catalog_search.selected().is_none());
        assert!(app.start().is_err());
        assert_eq!(app.navigator.active_id(), screen);
        match state {
            "settings" | "suspended" => {
                assert!(app.catalog.as_ref().unwrap().is_finished());
                if state == "settings" {
                    assert!(app.selection_view.is_none());
                    assert_eq!(
                        app.settings.as_ref().unwrap().profile.value(),
                        "direct-profile.bkp"
                    );
                    app.back();
                } else {
                    app.navigator.resume();
                }
                app.collect_catalog();
                assert!(app.catalog.is_none());
                assert_eq!(app.entries.len(), 1);
                assert_eq!(app.entries[0].path, path);
                assert_eq!(app.entries[0].title, "ÉTOILE 曲.bms");
                assert_eq!(app.catalog_search.indices().as_ref(), &[0]);
                assert!(app.title_font.as_ref().unwrap().get('曲').is_some());
            }
            "closing" => {
                assert!(app.catalog.is_none());
                assert!(app.closing());
                assert!(app.selection_view.is_none());
                app.collect_catalog();
                assert!(app.entries.is_empty());
                assert!(app.title_font.is_none());
            }
            _ => {
                assert!(app.catalog.is_none());
                assert!(
                    app.catalog_message
                        .as_deref()
                        .unwrap()
                        .contains("Catalog unavailable")
                );
                if state == "invalid" {
                    assert!(
                        app.catalog_message
                            .as_deref()
                            .unwrap()
                            .contains("invalid supplied font bytes")
                    );
                } else if state == "oversized" {
                    assert!(
                        app.catalog_message
                            .as_deref()
                            .unwrap()
                            .contains("font atlas configuration exceeds byte")
                    );
                }
                app.draw_selection().unwrap();
                app.scene.status().unwrap();
                assert!(app.hits.iter().all(|(id, _)| id.0 < 100));
                app.collect_catalog();
                assert!(app.entries.is_empty());
                assert!(app.title_font.is_none());
            }
        }
        assert!(app.game.is_none());
        assert_eq!(app.catalog_progress, player_chart::ScanProgress::default());
        let replacement = super::tests::lifecycle_fixture();
        assert_eq!(replacement.entries[0].title, "FIXTURE");
        assert!(replacement.catalog.is_none());
        assert!(replacement.title_font.is_none());
    }
}
