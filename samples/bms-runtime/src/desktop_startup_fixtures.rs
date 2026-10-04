//! Deferred profile startup fixtures; no window, GPU or native device is opened.
use super::*;
use beatkernel_bms_runtime::settings_profile::{encode_player_profile, encode_profile};
use std::{
    fs,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};

const WAIT: Duration = Duration::from_secs(5);

struct StartupFiles(PathBuf);
impl StartupFiles {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "beatkernel-startup-profile-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::write(
            root.join("library.bms"),
            "#TITLE Library original\n#ARTIST Library artist\n#BPM 60\n",
        )
        .unwrap();
        fs::write(root.join("title.ttf"), font_fixture::font_bytes()).unwrap();
        let files = Self(root);
        let model = fixture_profile(settings_host());
        fs::write(
            files.0.join("v2.bkp"),
            encode_player_profile(&model, settings_host()).unwrap(),
        )
        .unwrap();
        fs::write(
            files.0.join("v1.bkp"),
            encode_profile(&model.native, settings_host()).unwrap(),
        )
        .unwrap();
        files
    }
    fn chart(&self) -> PathBuf {
        self.0
            .join("absent-parent")
            .join("..")
            .join("ÉTOILE chart.bms")
    }
}
impl Drop for StartupFiles {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture_profile(host: SettingsHost) -> PlayerProfile {
    let mut native = vec![
        "--chart-seed".into(),
        "17".into(),
        "--early-ns".into(),
        "40000000".into(),
    ];
    let audio = match host {
        SettingsHost::Windows => ["--buffer", "frames:512", "--period", "frames:128"],
        SettingsHost::Linux | SettingsHost::Macos => ["--rate", "44100", "--buffer-frames", "1024"],
    };
    native.extend(audio.into_iter().map(str::to_owned));
    PlayerProfile {
        native: NativeSettings::from_args(&native, host).unwrap(),
        presentation: PresentationSettings {
            backend: BackendChoice::Vulkan,
            presentation: Presentation::Mailbox,
            fps: 90,
            lookahead_ms: 3456,
        },
    }
}

fn options(files: &StartupFiles, profile: &str, route: &str) -> Options {
    let (flag, path) = if route == "library" {
        ("--library", files.0.clone())
    } else {
        ("--chart", files.chart())
    };
    let mut args = vec![
        flag.into(),
        path.to_str().unwrap().into(),
        "--profile".into(),
        files.0.join(profile).to_str().unwrap().into(),
    ];
    if route == "font" {
        args.extend([
            "--title-font".into(),
            files.0.join("title.ttf").to_str().unwrap().into(),
        ]);
    }
    Options::parse(&args).unwrap()
}

fn native_value<'a>(options: &'a Options, flag: &str) -> Option<&'a str> {
    options
        .native
        .chunks_exact(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn gated_startup(
    options: Options,
) -> (
    NativeCatalog<Options>,
    mpsc::Receiver<(thread::ThreadId, CatalogControl)>,
    mpsc::SyncSender<()>,
) {
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let job = NativeCatalog::spawn_prepared(move |control| {
        entered
            .send((thread::current().id(), control.clone()))
            .unwrap();
        gate.recv_timeout(WAIT).map_err(|error| error.to_string())?;
        prepare_startup_options(options, control)
    })
    .unwrap();
    (job, ready, release)
}

fn gated_prepared_startup(
    options: Options,
) -> (
    NativeCatalog<Options>,
    mpsc::Receiver<(thread::ThreadId, CatalogControl)>,
    mpsc::SyncSender<()>,
) {
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let job = NativeCatalog::spawn_prepared(move |control| {
        let prepared = prepare_startup_options(options, control)?;
        entered
            .send((thread::current().id(), control.clone()))
            .unwrap();
        gate.recv_timeout(WAIT).map_err(|error| error.to_string())?;
        Ok(prepared)
    })
    .unwrap();
    (job, ready, release)
}

fn pending_app(options: Options, job: NativeCatalog<Options>) -> Desktop {
    attach_startup(super::tests::lifecycle_fixture(), options, job)
}

fn attach_startup(mut app: Desktop, options: Options, job: NativeCatalog<Options>) -> Desktop {
    app.options = options;
    app.startup = Some(job);
    app.entries.clear();
    app.selection_items = Arc::from([]);
    app.selection_diagnostics = Arc::from([]);
    app.catalog_search = CatalogSearch::new(&[]).unwrap();
    app
}

fn unpublished(app: &Desktop) {
    assert!(app.entries.is_empty());
    assert!(app.selection_items.is_empty());
    assert!(app.selection_diagnostics.is_empty());
    assert!(app.catalog_search.selected().is_none());
    assert!(app.selection_view.is_none());
    assert!(app.title_font.is_none());
    assert!(app.font_text.is_none());
    assert!(app.input_font.is_none());
    assert!(app.renderer.is_none());
    assert!(app.instance.is_none());
    assert!(app.catalog.is_none());
    assert!(app.game.is_none());
    assert!(app.profile_io.is_none());
}

fn wait_startup(app: &Desktop) {
    let deadline = Instant::now() + WAIT;
    while !app.startup.as_ref().unwrap().is_finished() {
        assert!(
            Instant::now() < deadline,
            "released startup worker did not finish"
        );
        thread::yield_now();
    }
}

#[test]
fn actual_profile_job_preserves_v1_v2_and_literal_selection_with_cli_native_and_display_precedence()
{
    let files = StartupFiles::new();
    let mut plain = options(&files, "missing.bkp", "direct");
    plain.profile = None;
    assert!(spawn_startup_profile(&plain).unwrap().is_none());

    for version in ["v1.bkp", "v2.bkp"] {
        for route in ["font", "library"] {
            let mut supplied = options(&files, version, route);
            supplied.native.extend(["--chart-seed".into(), "23".into()]);
            match settings_host() {
                SettingsHost::Windows => supplied
                    .native
                    .extend(["--buffer".into(), "frames:2048".into()]),
                SettingsHost::Linux | SettingsHost::Macos => supplied.native.extend([
                    "--rate".into(),
                    "48000".into(),
                    "--buffer-frames".into(),
                    "2048".into(),
                ]),
            }
            supplied.display_overrides = vec![
                "--gpu-backend".into(),
                "gl".into(),
                "--ui-fps".into(),
                "165".into(),
            ];
            supplied.set_display(
                PresentationSettings::default()
                    .apply_overrides(&supplied.display_overrides)
                    .unwrap(),
            );
            let original_chart = supplied.chart.clone();
            let original_library = supplied.library.clone();
            let original_font = supplied.title_font.clone();
            let original_profile = supplied.profile.clone();
            let mut job = spawn_startup_profile(&supplied).unwrap().unwrap();
            let prepared = job.join().unwrap().unwrap();
            assert_eq!(prepared.chart, original_chart);
            assert_eq!(prepared.library, original_library);
            assert_eq!(prepared.title_font, original_font);
            assert_eq!(prepared.profile, original_profile);
            assert_eq!(native_value(&prepared, "--chart"), None);
            assert_eq!(native_value(&prepared, "--chart-seed"), Some("23"));
            assert_eq!(native_value(&prepared, "--early-ns"), Some("40000000"));
            match settings_host() {
                SettingsHost::Windows => {
                    assert_eq!(native_value(&prepared, "--buffer"), Some("frames:2048"));
                    assert_eq!(native_value(&prepared, "--period"), Some("frames:128"));
                }
                SettingsHost::Linux | SettingsHost::Macos => {
                    assert_eq!(native_value(&prepared, "--rate"), Some("48000"));
                    assert_eq!(native_value(&prepared, "--buffer-frames"), Some("2048"));
                }
            }
            assert_eq!(prepared.backend, BackendChoice::Gl);
            assert_eq!(prepared.fps, 165);
            assert_eq!(
                prepared.presentation,
                if version == "v1.bkp" {
                    Presentation::Fifo
                } else {
                    Presentation::Mailbox
                }
            );
            assert_eq!(
                prepared.lookahead,
                if version == "v1.bkp" {
                    2_000_000_000
                } else {
                    3_456_000_000
                }
            );
            assert_eq!(job.progress(), player_chart::ScanProgress::default());
            assert!(job.poll().is_none());
            assert!(job.join().is_none());
            // Preparing a result never mutates the CLI source or owns chart/font I/O.
            assert_eq!(supplied.chart, original_chart);
            assert_eq!(supplied.library, original_library);
            assert_eq!(supplied.title_font, original_font);
            assert_eq!(native_value(&supplied, "--early-ns"), None);
        }
    }
}

#[test]
fn pending_profile_blocks_input_and_gpu_then_one_actual_join_begins_the_selected_preparation() {
    let files = StartupFiles::new();
    for route in ["direct", "font", "library"] {
        let supplied = options(&files, "v2.bkp", route);
        let (job, ready, release) = gated_startup(supplied.clone());
        let mut app = pending_app(supplied, job);
        let (worker, control) = ready.recv_timeout(WAIT).unwrap();
        assert_ne!(worker, thread::current().id());
        assert!(control.checkpoint().is_ok());
        unpublished(&app);
        assert_eq!(app.active_backend, BackendChoice::Auto);
        assert_eq!(app.options.backend, BackendChoice::Auto);
        assert!(app.startup_busy());
        assert!(!app.ui_ready());
        assert!(!app.reactive_waits_for_events());
        let screen = app.navigator.active_id();
        app.draw().unwrap();
        assert!(app.hits.is_empty());
        assert!(app.begin_selection().is_err());
        assert!(app.start().is_err());
        assert!(app.navigate(ScreenRoute::Settings).is_err());
        app.open_settings();
        app.set_search_focus(true);
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
        assert!(app.settings.is_none());
        assert!(!app.search_focused);
        assert_eq!(app.search_editor.value(), "");
        assert_eq!(app.navigator.active_id(), screen);
        app.collect_startup();
        unpublished(&app);
        assert!(!app.startup.as_ref().unwrap().is_finished());
        release.send(()).unwrap();
        wait_startup(&app);
        assert!(
            app.startup_busy(),
            "finished but unconsumed startup must still wake its active owner"
        );
        assert_eq!(app.options.backend, BackendChoice::Auto);
        unpublished(&app);
        app.collect_startup();
        assert!(app.startup.is_none());
        assert!(!app.startup_busy());
        assert_eq!(app.active_backend, BackendChoice::Vulkan);
        assert_eq!(app.options.backend, BackendChoice::Vulkan);
        assert_eq!(app.options.presentation, Presentation::Mailbox);
        assert_eq!(app.options.fps, 90);
        assert_eq!(app.options.lookahead, 3_456_000_000);
        assert_eq!(native_value(&app.options, "--chart-seed"), Some("17"));
        assert!(app.renderer.is_none());
        assert!(app.instance.is_none());
        assert!(app.profile_io.is_none());
        assert!(app.game.is_none());
        assert!(app.fatal.is_none());
        assert_eq!(app.navigator.active_id(), screen);
        if route == "direct" {
            assert!(app.catalog.is_none());
            assert_eq!(app.entries.len(), 1);
            assert_eq!(app.entries[0].path, files.chart());
            assert_eq!(app.entries[0].title, "ÉTOILE chart.bms");
            assert_eq!(app.entries[0].artist, "");
            assert!(app.title_font.is_none());
        } else {
            assert!(app.catalog.is_some());
            assert!(app.entries.is_empty());
            assert!(app.title_font.is_none());
            assert!(app.start().is_err());
            let deadline = Instant::now() + WAIT;
            while !app.catalog.as_ref().unwrap().is_finished() {
                assert!(Instant::now() < deadline);
                thread::yield_now();
            }
            app.collect_catalog();
            assert!(app.catalog.is_none());
            assert_eq!(app.entries.len(), 1);
            if route == "font" {
                assert_eq!(app.entries[0].path, files.chart());
                assert_eq!(app.entries[0].title, "ÉTOILE chart.bms");
                assert!(app.title_font.as_ref().unwrap().get('É').is_some());
            } else {
                assert_eq!(app.entries[0].path, files.0.join("library.bms"));
                assert_eq!(app.entries[0].title, "Library original");
                assert_eq!(app.entries[0].artist, "Library artist");
                assert!(app.title_font.is_none());
            }
        }
        let retained = app.selection_items.clone();
        for _ in 0..3 {
            app.collect_startup();
            assert!(Arc::ptr_eq(&retained, &app.selection_items));
            assert_eq!(app.active_backend, BackendChoice::Vulkan);
        }
        assert!(app.reactive_waits_for_events());
        assert!(app.game.is_none());
    }
}

#[test]
fn hidden_suspended_occluded_and_cancelled_startup_owners_cannot_install_or_revive_another_session()
{
    let files = StartupFiles::new();
    for state in ["hidden", "suspended", "occluded", "close", "escape", "drop"] {
        let mut base = super::tests::lifecycle_fixture();
        if state == "hidden" {
            base.open_settings();
            base.settings
                .as_mut()
                .unwrap()
                .profile
                .insert("retained-draft.bkp")
                .unwrap();
        } else if state == "suspended" {
            base.navigator.suspend();
        } else if state == "occluded" {
            base.occluded = true;
        }
        let supplied = options(&files, "v2.bkp", "direct");
        let (job, ready, release) = if matches!(state, "close" | "escape" | "drop") {
            // A genuine decoded/overlaid result exists on the worker already.
            // Cancellation must also defeat its delayed return, not just a
            // checkpoint before any profile has been read.
            gated_prepared_startup(supplied.clone())
        } else {
            gated_startup(supplied.clone())
        };
        let mut app = attach_startup(base, supplied, job);
        let (_, control) = ready.recv_timeout(WAIT).unwrap();
        let screen = app.navigator.active_id();
        if state == "close" {
            app.request_close();
        } else if state == "escape" {
            app.keyboard_input(
                PhysicalKey::Code(KeyCode::Escape),
                &Key::Named(winit::keyboard::NamedKey::Escape),
                None,
                false,
            );
        }
        if matches!(state, "close" | "escape") {
            assert!(app.closing());
            assert!(control.is_cancelled());
            assert!(!app.startup.as_ref().unwrap().is_finished());
        }
        unpublished(&app);
        release.send(()).unwrap();
        if state == "drop" {
            drop(app); // Drops/cancels/joins the actual owner; there is no detached result callback.
            assert!(control.is_cancelled());
            assert!(control.checkpoint().is_err());
        } else {
            wait_startup(&app);
            if matches!(state, "close" | "escape") {
                assert!(
                    app.startup_busy(),
                    "Closing must consume the completed owner before it can idle"
                );
            }
            app.collect_startup();
            unpublished(&app);
            assert_eq!(app.options.backend, BackendChoice::Auto);
            assert_eq!(app.active_backend, BackendChoice::Auto);
            assert_eq!(app.options.fps, 120);
            match state {
                "hidden" | "suspended" | "occluded" => {
                    assert!(app.startup.as_ref().unwrap().is_finished());
                    assert!(!app.startup_busy());
                    assert_eq!(app.navigator.active_id(), screen);
                    if state == "hidden" {
                        assert_eq!(
                            app.settings.as_ref().unwrap().profile.value(),
                            "retained-draft.bkp"
                        );
                        // Exercise the collector's defensive hidden-screen gate
                        // through the real Navigator. UI navigation while startup
                        // is pending is separately refused by the previous group.
                        let mut next = app.navigator.clone();
                        next.back(false, false).unwrap();
                        app.commit_route(next);
                    } else if state == "suspended" {
                        app.navigator.resume();
                    } else {
                        app.occluded = false;
                    }
                    assert!(app.startup_busy());
                    app.collect_startup();
                    assert!(app.startup.is_none());
                    assert_eq!(app.active_backend, BackendChoice::Vulkan);
                    assert_eq!(app.entries.len(), 1);
                    assert_eq!(app.entries[0].path, files.chart());
                    let items = app.selection_items.clone();
                    app.collect_startup();
                    assert!(Arc::ptr_eq(&items, &app.selection_items));
                }
                "close" | "escape" => {
                    assert!(app.startup.is_none());
                    assert!(app.closing());
                    assert!(
                        app.fatal.is_none(),
                        "ordinary cancellation is not a startup failure"
                    );
                    app.collect_startup();
                    unpublished(&app);
                }
                _ => unreachable!(),
            }
        }
        let replacement = super::tests::lifecycle_fixture();
        assert!(replacement.startup.is_none());
        assert_eq!(replacement.entries[0].title, "FIXTURE");
        assert_eq!(replacement.active_backend, BackendChoice::Auto);
        assert!(replacement.fatal.is_none());
        assert!(replacement.catalog.is_none());
        assert!(replacement.title_font.is_none());
    }
}

#[test]
fn missing_foreign_malformed_and_oversized_startup_profiles_are_fatal_without_default_selection() {
    let files = StartupFiles::new();
    let other_host = if settings_host() == SettingsHost::Linux {
        SettingsHost::Windows
    } else {
        SettingsHost::Linux
    };
    fs::write(
        files.0.join("foreign.bkp"),
        encode_player_profile(&fixture_profile(other_host), other_host).unwrap(),
    )
    .unwrap();
    fs::write(files.0.join("malformed.bkp"), b"not a profile\n").unwrap();
    fs::File::create(files.0.join("oversized.bkp"))
        .unwrap()
        .set_len(72 * 1024 + 1)
        .unwrap();
    fs::create_dir(files.0.join("directory.bkp")).unwrap();
    for name in [
        "missing.bkp",
        "foreign.bkp",
        "malformed.bkp",
        "oversized.bkp",
        "directory.bkp",
    ] {
        let supplied = options(&files, name, "font");
        let original_native = supplied.native.clone();
        let job = spawn_startup_profile(&supplied).unwrap().unwrap();
        let mut app = pending_app(supplied, job);
        unpublished(&app);
        wait_startup(&app);
        assert!(app.startup.is_some());
        assert!(app.fatal.is_none());
        assert!(app.start().is_err());
        app.collect_startup();
        assert!(app.startup.is_none());
        assert!(!app.startup_busy());
        assert!(app.closing());
        assert!(!app.fatal.as_deref().unwrap().is_empty());
        if matches!(name, "foreign.bkp" | "malformed.bkp") {
            assert!(
                app.fatal
                    .as_deref()
                    .unwrap()
                    .contains("profile magic, version or operating system differs")
            );
        } else if name == "oversized.bkp" {
            assert!(app.fatal.as_deref().unwrap().contains("72 KiB"));
        }
        assert_eq!(app.options.native, original_native);
        assert_eq!(app.options.profile, Some(files.0.join(name)));
        assert_eq!(app.options.chart, Some(files.chart()));
        assert_eq!(app.options.title_font, Some(files.0.join("title.ttf")));
        assert_eq!(app.active_backend, BackendChoice::Auto);
        assert_eq!(app.options.backend, BackendChoice::Auto);
        unpublished(&app);
        let fatal = app.fatal.clone();
        app.collect_startup();
        app.collect_catalog();
        app.open_settings();
        assert!(app.start().is_err());
        assert_eq!(app.fatal, fatal);
        assert!(app.settings.is_none());
        unpublished(&app);
    }
}
