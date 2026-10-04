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
