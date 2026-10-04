//! Deferred owned renderer startup fixtures. No window or successful GPU is fabricated.
use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};

const WAIT: Duration = Duration::from_secs(5);

struct WorkerTail(Arc<AtomicUsize>);
impl Drop for WorkerTail {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn gated_renderer(
    error: &'static str,
) -> (
    RendererStartup,
    CatalogControl,
    mpsc::SyncSender<()>,
    Arc<AtomicUsize>,
) {
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let finished = Arc::new(AtomicUsize::new(0));
    let tail = finished.clone();
    let job = NativeCatalog::<PreparedRenderer>::spawn_prepared(move |control| {
        let _tail = WorkerTail(tail);
        entered
            .send((thread::current().id(), control.clone()))
            .unwrap();
        gate.recv_timeout(WAIT).map_err(|error| error.to_string())?;
        Err(error.into())
    })
    .unwrap();
    let (worker, control) = ready.recv_timeout(WAIT).unwrap();
    assert_ne!(worker, thread::current().id());
    assert!(!job.is_finished());
    (
        RendererStartup {
            job,
            retired: false,
        },
        control,
        release,
        finished,
    )
}

fn wait_renderer(app: &Desktop) {
    let deadline = Instant::now() + WAIT;
    while !app.renderer_startup.as_ref().unwrap().job.is_finished() {
        assert!(
            Instant::now() < deadline,
            "released renderer worker did not finish"
        );
        thread::yield_now();
    }
}

fn no_graphics_or_game(app: &Desktop) {
    assert!(app.window.is_none());
    assert!(app.renderer.is_none());
    assert!(app.instance.is_none());
    assert!(app.font_text.is_none());
    assert!(app.input_font.is_none());
    assert!(app.game.is_none());
}

#[test]
fn pending_renderer_fences_real_desktop_actions_and_catalog_publication_until_joined_failure() {
    let mut app = super::tests::lifecycle_fixture();
    app.set_search_focus(true);
    app.draw_selection().unwrap();
    let old_hits = app.hits.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    assert!(old_hits.contains(&ControlId(1)));
    assert!(old_hits.contains(&ControlId(5)));
    let screen = app.navigator.active_id();
    let items = app.selection_items.clone();
    let (startup, control, release, tail) = gated_renderer("fixture adapter preparation failed");
    app.renderer_startup = Some(startup);
    assert!(app.catalog.is_none());
    assert!(
        app.start().is_err(),
        "renderer ownership alone fences gameplay startup"
    );

    // This is the real independent catalog preparation, with no filesystem or
    // GPU prerequisite. Its ready result must remain owned behind the renderer.
    app.catalog = Some(
        NativeCatalog::spawn_prepared(|control| {
            prepare_catalog(
                player_chart::ChartLibrary {
                    entries: vec![player_chart::LibraryEntry {
                        path: PathBuf::from("next.bms"),
                        title: "UNPUBLISHED NEXT CHART".into(),
                        artist: "NEXT ARTIST".into(),
                    }],
                    diagnostics: vec!["UNPUBLISHED DIAGNOSTIC".into()],
                },
                None,
                control,
            )
        })
        .unwrap(),
    );
    let deadline = Instant::now() + WAIT;
    while !app.catalog.as_ref().unwrap().is_finished() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(app.renderer_pending());
    assert!(app.renderer_startup_busy());
    assert!(!app.ui_ready());
    assert!(!app.reactive_waits_for_events());
    assert!(app.start().is_err());
    assert!(app.navigate(ScreenRoute::Settings).is_err());
    app.open_settings();
    for id in old_hits {
        app.activate(id);
    }
    for key in [KeyCode::F2, KeyCode::F3, KeyCode::Enter, KeyCode::Tab] {
        app.key(key, false);
    }
    app.keyboard_input(
        PhysicalKey::Code(KeyCode::KeyA),
        &Key::Character("a".into()),
        Some("a"),
        false,
    );
    app.key(KeyCode::Escape, true);
    assert!(!app.closing());
    assert_eq!(app.navigator.active_id(), screen);
    assert!(app.settings.is_none());
    assert_eq!(app.search_editor.value(), "");
    assert!(app.hit().is_none());
    app.selection_view = None;
    app.hits.clear();
    app.draw().unwrap();
    assert!(app.selection_view.is_none());
    assert!(app.hits.is_empty());
    for _ in 0..3 {
        app.collect_renderer();
        app.collect_catalog();
        assert!(Arc::ptr_eq(&items, &app.selection_items));
        assert_eq!(app.entries[0].title, "FIXTURE");
        assert!(app.selection_diagnostics.is_empty());
        assert!(app.catalog.as_ref().unwrap().is_finished());
        assert!(app.fatal.is_none());
        no_graphics_or_game(&app);
    }
    assert!(!control.is_cancelled());
    assert_eq!(tail.load(Ordering::SeqCst), 0);
    release.send(()).unwrap();
    wait_renderer(&app);
    assert!(
        app.renderer_startup_busy(),
        "ready ownership still requires collection"
    );
    assert!(
        app.fatal.is_none(),
        "thread completion alone is not UI adoption"
    );
    app.collect_renderer();
    assert!(app.renderer_startup.is_none());
    assert_eq!(
        app.fatal.as_deref(),
        Some("fixture adapter preparation failed")
    );
    assert!(app.closing());
    assert_eq!(tail.load(Ordering::SeqCst), 1);
    app.collect_catalog();
    assert!(app.catalog.is_none());
    assert!(Arc::ptr_eq(&items, &app.selection_items));
    for _ in 0..3 {
        app.collect_renderer();
        assert_eq!(
            app.fatal.as_deref(),
            Some("fixture adapter preparation failed")
        );
        assert!(!app.renderer_startup_busy());
        no_graphics_or_game(&app);
    }
}

#[test]
fn ready_renderer_error_is_retained_while_ineligible_and_not_restricted_to_selection_or_focus() {
    for hidden in ["occluded", "suspended"] {
        let mut app = super::tests::lifecycle_fixture();
        app.open_settings();
        app.settings
            .as_mut()
            .unwrap()
            .profile
            .insert("retained-profile.bkp")
            .unwrap();
        let screen = app.navigator.active_id();
        let (startup, control, release, tail) = gated_renderer("fixture device unavailable");
        app.renderer_startup = Some(startup);
        if hidden == "occluded" {
            app.occluded = true;
        } else {
            // Isolate the collector's defensive eligibility check. The real
            // suspend retirement method is exercised separately below.
            app.navigator.suspend();
        }
        release.send(()).unwrap();
        wait_renderer(&app);
        for _ in 0..3 {
            app.collect_renderer();
            assert!(app.renderer_startup.as_ref().unwrap().job.is_finished());
            assert!(
                !app.renderer_startup_busy(),
                "a hidden ready owner can wait for events"
            );
            assert!(!control.is_cancelled());
            assert!(app.fatal.is_none());
            assert!(!app.closing());
            assert_eq!(app.navigator.active_id(), screen);
            assert_eq!(
                app.settings.as_ref().unwrap().profile.value(),
                "retained-profile.bkp"
            );
            no_graphics_or_game(&app);
        }
        assert_eq!(tail.load(Ordering::SeqCst), 1);
        app.occluded = false;
        app.navigator.resume();
        app.active = false; // Focus is not surface eligibility; Settings also needs its renderer.
        assert!(app.renderer_startup_busy());
        app.collect_renderer();
        assert!(app.renderer_startup.is_none());
        assert_eq!(app.fatal.as_deref(), Some("fixture device unavailable"));
        assert!(app.closing());
        assert_eq!(tail.load(Ordering::SeqCst), 1);
        no_graphics_or_game(&app);
    }
}

#[test]
fn retired_renderer_generation_is_joined_and_discarded_before_replacement_even_after_resume() {
    for resume_before_join in [false, true] {
        let mut app = super::tests::lifecycle_fixture();
        let (old, old_control, release_old, old_tail) = gated_renderer("obsolete adapter failure");
        app.renderer_startup = Some(old);
        app.retire_renderer_startup();
        app.navigator.suspend();
        app.occluded = true;
        assert!(old_control.is_cancelled());
        assert!(app.renderer_startup.as_ref().unwrap().retired);
        app.collect_renderer();
        assert!(!app.renderer_startup.as_ref().unwrap().job.is_finished());
        if resume_before_join {
            app.navigator.resume();
            app.occluded = false;
        }
        app.initialize_renderer().unwrap();
        assert!(app.renderer_startup.as_ref().unwrap().retired);
        assert_eq!(old_tail.load(Ordering::SeqCst), 0);
        release_old.send(()).unwrap();
        wait_renderer(&app);
        assert!(
            app.renderer_startup_busy(),
            "retirement must join even while hidden"
        );
        app.collect_renderer();
        assert!(app.renderer_startup.is_none());
        assert!(app.fatal.is_none());
        assert!(!app.closing());
        assert_eq!(old_tail.load(Ordering::SeqCst), 1);
        no_graphics_or_game(&app);

        app.navigator.resume();
        app.occluded = false;
        let (fresh, fresh_control, release_fresh, fresh_tail) =
            gated_renderer("new surface failure");
        app.renderer_startup = Some(fresh);
        for _ in 0..3 {
            app.collect_renderer();
            assert!(!app.renderer_startup.as_ref().unwrap().retired);
            assert!(!fresh_control.is_cancelled());
            assert!(app.fatal.is_none());
        }
        release_fresh.send(()).unwrap();
        wait_renderer(&app);
        app.collect_renderer();
        assert_eq!(app.fatal.as_deref(), Some("new surface failure"));
        assert!(app.closing());
        assert_eq!(old_tail.load(Ordering::SeqCst), 1);
        assert_eq!(fresh_tail.load(Ordering::SeqCst), 1);
        no_graphics_or_game(&app);
    }
}

#[test]
fn close_escape_and_unexpected_desktop_drop_cancel_and_join_the_real_renderer_owner() {
    for escape in [false, true] {
        let mut app = super::tests::lifecycle_fixture();
        let (startup, control, release, tail) = gated_renderer("late error after close");
        app.renderer_startup = Some(startup);
        if escape {
            app.keyboard_input(
                PhysicalKey::Code(KeyCode::Escape),
                &Key::Named(winit::keyboard::NamedKey::Escape),
                None,
                false,
            );
        } else {
            app.request_close();
        }
        assert!(app.closing());
        assert!(control.is_cancelled());
        assert!(app.renderer_startup.as_ref().unwrap().retired);
        app.occluded = true;
        app.collect_renderer();
        assert!(app.renderer_startup.is_some());
        assert_eq!(tail.load(Ordering::SeqCst), 0);
        release.send(()).unwrap();
        wait_renderer(&app);
        assert!(app.renderer_startup_busy());
        app.collect_renderer();
        assert!(app.renderer_startup.is_none());
        assert!(app.fatal.is_none());
        assert_eq!(tail.load(Ordering::SeqCst), 1);
        app.navigator.resume();
        app.occluded = false;
        app.initialize_renderer().unwrap();
        app.collect_renderer();
        assert!(app.closing());
        assert!(app.renderer_startup.is_none());
        no_graphics_or_game(&app);
    }

    let (entered, ready) = mpsc::sync_channel(1);
    let finished = Arc::new(AtomicUsize::new(0));
    let tail = finished.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let observed = cancelled.clone();
    let job = NativeCatalog::<PreparedRenderer>::spawn_prepared(move |control| {
        let _tail = WorkerTail(tail);
        entered.send(control.clone()).unwrap();
        let deadline = Instant::now() + WAIT;
        while !control.is_cancelled() {
            if Instant::now() >= deadline {
                return Err("fixture cancellation was not delivered".into());
            }
            thread::yield_now();
        }
        observed.store(true, Ordering::SeqCst);
        Err("actual worker observed unexpected-owner-drop cancellation".into())
    })
    .unwrap();
    let control = ready.recv_timeout(WAIT).unwrap();
    let mut abandoned = super::tests::lifecycle_fixture();
    abandoned.renderer_startup = Some(RendererStartup {
        job,
        retired: false,
    });
    assert_eq!(finished.load(Ordering::SeqCst), 0);
    drop(abandoned); // Actual NativeCatalog Drop cancels and joins; no detached callback.
    assert!(control.is_cancelled());
    assert!(cancelled.load(Ordering::SeqCst));
    assert_eq!(finished.load(Ordering::SeqCst), 1);
    let mut replacement = super::tests::lifecycle_fixture();
    replacement.collect_renderer();
    assert!(!replacement.closing());
    assert!(replacement.fatal.is_none());
    assert!(replacement.renderer_startup.is_none());
    no_graphics_or_game(&replacement);
}
