//! Actual desktop clipboard routing with an injected backend and no native services.
use super::tests::lifecycle_fixture;
use super::*;
use crate::desktop_clipboard::TextClipboard;
use beatkernel_bms_runtime::ui::clipboard::ClipboardRequest;
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Call {
    Read,
    Write(String),
}
struct FakeClipboard {
    calls: Arc<Mutex<Vec<Call>>>,
    read: Result<String, String>,
    write: Result<(), String>,
}
impl TextClipboard for FakeClipboard {
    fn read_text(&mut self) -> Result<String, String> {
        self.calls.lock().unwrap().push(Call::Read);
        self.read.clone()
    }
    fn write_text(&mut self, value: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(Call::Write(value.into()));
        self.write.clone()
    }
}
fn install(
    app: &mut Desktop,
    read: Result<String, String>,
    write: Result<(), String>,
) -> Arc<Mutex<Vec<Call>>> {
    assert!(app.clipboard.is_none());
    let calls = Arc::new(Mutex::new(Vec::new()));
    app.clipboard = Some(
        ClipboardWorker::spawn(FakeClipboard {
            calls: Arc::clone(&calls),
            read,
            write,
        })
        .unwrap(),
    );
    calls
}
fn field_app(field: usize) -> Desktop {
    let mut app = lifecycle_fixture();
    if field == 0 {
        app.set_search_focus(true);
    } else {
        app.open_settings();
        match field {
            1 => {
                let draft = app.settings.as_mut().unwrap();
                let index = draft
                    .values
                    .fields()
                    .iter()
                    .position(|row| row.flag == "--record-replay")
                    .unwrap();
                draft.select(index).unwrap();
            }
            2 => app.settings.as_mut().unwrap().profile_focused = true,
            3 => {
                app.open_display();
                app.display.as_mut().unwrap().selected = 2;
            }
            4 | 5 => {
                app.open_practice();
                app.practice.as_mut().unwrap().end_focused = field == 5;
            }
            6 => app.open_records(),
            _ => unreachable!(),
        }
    }
    app.sync_ime();
    assert!(app.text_target().is_some(), "field {field} did not open");
    app
}
fn editor(app: &Desktop) -> &LineEditor {
    app.text_editor(app.text_target().unwrap().field).unwrap()
}
fn selected(app: &mut Desktop, value: &str) {
    let field = app.text_target().unwrap().field;
    app.text_editor_mut(field).unwrap().select_all();
    app.modifiers_changed(ModifiersState::empty());
    app.keyboard_input(
        PhysicalKey::Code(KeyCode::KeyQ),
        &Key::Character(value.into()),
        Some(value),
        false,
    );
    assert_eq!(editor(app).value(), value);
    app.text_editor_mut(field).unwrap().select_all();
    app.invalidate_hits();
}
fn shortcut(app: &mut Desktop, letter: &str, repeat: bool) {
    app.modifiers_changed(if cfg!(target_os = "macos") {
        ModifiersState::SUPER
    } else {
        ModifiersState::CONTROL
    });
    // The physical key intentionally differs from the logical shortcut. The
    // supplied ordinary text must be consumed before any editor insertion.
    app.keyboard_input(
        PhysicalKey::Code(KeyCode::KeyQ),
        &Key::Character(letter.into()),
        Some("must not be inserted"),
        repeat,
    );
}
fn drain(app: &mut Desktop) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.clipboard_busy() {
        app.collect_clipboard();
        assert!(Instant::now() < deadline, "fake clipboard did not finish");
        thread::yield_now();
    }
}
fn error(app: &Desktop) -> Option<&str> {
    match app.navigator.route() {
        ScreenRoute::Selection => app.failure.as_deref(),
        ScreenRoute::Settings => app.settings.as_ref().unwrap().error.as_deref(),
        ScreenRoute::Display => app.display.as_ref().unwrap().error.as_deref(),
        ScreenRoute::Practice => app.practice.as_ref().unwrap().error.as_deref(),
        ScreenRoute::Records => app.records.as_ref().unwrap().error.as_deref(),
        _ => None,
    }
}

#[test]
fn actual_clipboard_keys_copy_cut_and_paste_each_of_the_seven_editable_targets() {
    for field in 0..7 {
        let mut app = field_app(field);
        let calls = install(&mut app, Ok("音é".into()), Ok(()));
        selected(&mut app, "a別b");
        let baseline = editor(&app).clone();
        let model = app
            .settings
            .as_ref()
            .map(|draft| draft.values.native_args());
        shortcut(&mut app, "C", false);
        assert!(app.pending_clipboard.is_some());
        assert!(app.clipboard_busy());
        assert_eq!(editor(&app), &baseline);
        shortcut(&mut app, "C", true);
        assert!(app.pending_clipboard.is_some());
        drain(&mut app);
        assert_eq!(editor(&app), &baseline);
        assert_eq!(*calls.lock().unwrap(), [Call::Write("a別b".into())]);

        shortcut(&mut app, "x", false);
        assert_eq!(editor(&app), &baseline);
        assert_eq!(
            app.settings
                .as_ref()
                .map(|draft| draft.values.native_args()),
            model
        );
        drain(&mut app);
        assert_eq!(editor(&app).value(), "");
        assert_eq!(editor(&app).selection(), None);
        if field == 0 {
            assert_eq!(&*app.catalog_search.indices(), &[0]);
        }
        if field == 1 {
            let draft = app.settings.as_ref().unwrap();
            assert_eq!(draft.values.fields()[draft.selected].value, "");
        }

        shortcut(&mut app, "V", false);
        assert_eq!(editor(&app).value(), "");
        drain(&mut app);
        assert_eq!((editor(&app).value(), editor(&app).cursor()), ("音é", 5));
        assert_eq!(editor(&app).selection(), None);
        if field == 0 {
            assert!(app.catalog_search.indices().is_empty());
        } else if field == 1 {
            let draft = app.settings.as_ref().unwrap();
            assert_eq!(draft.values.fields()[draft.selected].value, "音é");
        } else {
            assert_eq!(
                app.settings
                    .as_ref()
                    .map(|draft| draft.values.native_args()),
                model
            );
        }
        assert_eq!(
            *calls.lock().unwrap(),
            [
                Call::Write("a別b".into()),
                Call::Write("a別b".into()),
                Call::Read
            ]
        );
        assert!(error(&app).is_none());
        assert!(app.pending_clipboard.is_none());
        assert_eq!(app.options.native, ["--chart", "fixture.bms"]);
        assert!(app.window.is_none() && app.renderer.is_none() && app.game.is_none());
    }
}

#[test]
fn clipboard_commands_require_the_exact_platform_modifier_and_logical_character() {
    for macos in [false, true] {
        let command = if macos {
            ModifiersState::SUPER
        } else {
            ModifiersState::CONTROL
        };
        let other = if macos {
            ModifiersState::CONTROL
        } else {
            ModifiersState::SUPER
        };
        for (letter, action) in [
            ("c", ClipboardAction::Copy),
            ("C", ClipboardAction::Copy),
            ("x", ClipboardAction::Cut),
            ("X", ClipboardAction::Cut),
            ("v", ClipboardAction::Paste),
            ("V", ClipboardAction::Paste),
        ] {
            let logical = Key::Character(letter.into());
            assert_eq!(clipboard_command(&logical, command, macos), Some(action));
            for modifiers in [
                ModifiersState::empty(),
                other,
                command | other,
                command | ModifiersState::SHIFT,
                command | ModifiersState::ALT,
            ] {
                assert_eq!(clipboard_command(&logical, modifiers, macos), None);
            }
        }
        for letter in ["q", "cv", "ç", ""] {
            assert_eq!(
                clipboard_command(&Key::Character(letter.into()), command, macos),
                None
            );
        }
    }
}

#[test]
fn repeat_no_selection_composition_and_unavailable_routes_do_not_submit() {
    let mut app = field_app(0);
    let calls = install(&mut app, Ok("reply".into()), Ok(()));
    selected(&mut app, "base");
    let baseline = editor(&app).clone();
    for letter in ["c", "x", "v"] {
        shortcut(&mut app, letter, true);
        assert!(!app.clipboard_busy());
        assert_eq!(editor(&app), &baseline);
    }
    app.search_editor.clear_selection();
    let baseline = app.search_editor.clone();
    for letter in ["c", "x"] {
        shortcut(&mut app, letter, false);
        assert_eq!(app.search_editor, baseline);
        assert!(!app.clipboard_busy());
    }
    app.ime_event(Ime::Enabled);
    app.ime_event(Ime::Preedit("音".into(), None));
    let preview = app.ime.preview.clone();
    for letter in ["c", "x", "v"] {
        shortcut(&mut app, letter, false);
        assert_eq!(app.search_editor, baseline);
        assert_eq!(app.ime.preview, preview);
        assert!(!app.clipboard_busy());
    }
    app.ime_event(Ime::Disabled);
    app.set_search_focus(false);
    shortcut(&mut app, "v", false);
    assert!(app.text_target().is_none());
    app.set_search_focus(true);
    app.active = false;
    shortcut(&mut app, "v", false);
    assert_eq!(app.search_editor, baseline);
    assert!(!app.clipboard_busy());
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn clipboard_failures_preserve_editors_and_settings_and_report_to_the_current_route() {
    for (field, payload) in [
        (0, "x".repeat(257)),
        (1, "bad\nvalue".into()),
        (2, "🙂".repeat(1025)),
        (3, "x".repeat(33)),
        (4, "x".repeat(65)),
        (5, "x".repeat(65)),
        (6, "\u{2028}".into()),
    ] {
        let mut app = field_app(field);
        install(&mut app, Ok(payload), Ok(()));
        selected(&mut app, "a別b");
        let baseline = editor(&app).clone();
        let model = app
            .settings
            .as_ref()
            .map(|draft| draft.values.native_args());
        shortcut(&mut app, "v", false);
        drain(&mut app);
        assert_eq!(editor(&app), &baseline);
        assert_eq!(
            app.settings
                .as_ref()
                .map(|draft| draft.values.native_args()),
            model
        );
        assert!(error(&app).is_some(), "field {field} lost its error");
    }
    for letter in ["x", "v"] {
        let mut app = field_app(1);
        install(
            &mut app,
            Err("fake read failure".into()),
            Err("fake write failure".into()),
        );
        selected(&mut app, "saved.bkr");
        let baseline = editor(&app).clone();
        let model = app.settings.as_ref().unwrap().values.native_args();
        shortcut(&mut app, letter, false);
        drain(&mut app);
        assert_eq!(editor(&app), &baseline);
        assert_eq!(app.settings.as_ref().unwrap().values.native_args(), model);
        assert!(error(&app).unwrap().contains("fake"));
    }
    for (action, reply) in [
        (ClipboardAction::Copy, Some("wrong write payload".into())),
        (ClipboardAction::Cut, Some("wrong write payload".into())),
        (ClipboardAction::Paste, None),
    ] {
        let mut app = field_app(1);
        install(&mut app, Ok("real read".into()), Ok(()));
        selected(&mut app, "preserved");
        let baseline = editor(&app).clone();
        let model = app.settings.as_ref().unwrap().values.native_args();
        app.begin_clipboard(action).unwrap();
        app.finish_clipboard(Ok(reply));
        assert!(error(&app).is_some());
        // The real fake-worker reply is still drained, with no pending edit to publish.
        drain(&mut app);
        assert_eq!(editor(&app), &baseline);
        assert_eq!(app.settings.as_ref().unwrap().values.native_args(), model);
    }
}

#[test]
fn paste_revalidates_aggregate_settings_including_other_rows_changed_during_read() {
    fn fill_other_rows(app: &mut Desktop) {
        let draft = app.settings.as_mut().unwrap();
        let mut remaining = 65_534;
        for index in 0..draft.values.fields().len() {
            if index == draft.selected {
                continue;
            }
            let count = remaining.min(4096);
            draft.values.set_value(index, &"x".repeat(count)).unwrap();
            remaining -= count;
            if remaining == 0 {
                break;
            }
        }
        assert_eq!(remaining, 0);
        assert_eq!(
            draft
                .values
                .fields()
                .iter()
                .map(|row| row.value.len())
                .sum::<usize>(),
            65_535
        );
    }
    for changed_while_pending in [false, true] {
        let mut app = field_app(1);
        install(&mut app, Ok("abc".into()), Ok(()));
        selected(&mut app, "a");
        if !changed_while_pending {
            fill_other_rows(&mut app);
        }
        let baseline = editor(&app).clone();
        app.begin_clipboard(ClipboardAction::Paste).unwrap();
        if changed_while_pending {
            fill_other_rows(&mut app);
        }
        app.sync_clipboard();
        assert!(app.pending_clipboard.is_some());
        let before = app.settings.as_ref().unwrap().values.fields().to_vec();
        drain(&mut app);
        assert_eq!(editor(&app), &baseline);
        assert_eq!(app.settings.as_ref().unwrap().values.fields(), before);
        assert!(error(&app).unwrap().contains("64 KiB"));
    }
}

#[test]
fn cancelled_requests_stay_busy_until_drained_and_closing_releases_the_fake_owner() {
    let mut app = field_app(2);
    let calls = install(&mut app, Ok("音".into()), Ok(()));
    selected(&mut app, "base");
    app.begin_clipboard(ClipboardAction::Paste).unwrap();
    assert!(app.begin_clipboard(ClipboardAction::Paste).is_err());
    assert!(app.pending_clipboard.is_some());
    app.text_editor_mut(TextField::Profile).unwrap().left();
    app.invalidate_hits();
    assert!(app.pending_clipboard.is_none());
    assert!(app.clipboard_busy());
    assert!(app.begin_clipboard(ClipboardAction::Paste).is_err());
    assert!(app.pending_clipboard.is_none());
    drain(&mut app);
    assert_eq!(editor(&app).value(), "base");
    app.begin_clipboard(ClipboardAction::Paste).unwrap();
    drain(&mut app);
    assert_eq!(editor(&app).value(), "音base");
    assert_eq!(*calls.lock().unwrap(), [Call::Read, Call::Read]);

    selected(&mut app, "close");
    app.begin_clipboard(ClipboardAction::Paste).unwrap();
    app.request_close();
    assert_eq!(app.navigator.route(), ScreenRoute::Closing);
    assert!(app.pending_clipboard.is_none());
    assert!(
        app.clipboard
            .as_mut()
            .unwrap()
            .submit(ClipboardRequest::Read)
            .is_err()
    );
    drain(&mut app);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.clipboard.as_ref().unwrap().is_finished() {
        assert!(Instant::now() < deadline, "fake owner did not close");
        thread::yield_now();
    }
    assert_eq!(*calls.lock().unwrap(), [Call::Read, Call::Read, Call::Read]);
    app.clipboard = None;
}

#[test]
fn text_caret_and_selection_changes_cancel_even_after_the_exact_editor_is_restored() {
    for change in 0..3 {
        let mut app = field_app(2);
        install(&mut app, Ok("obsolete".into()), Ok(()));
        selected(&mut app, "base");
        let baseline = editor(&app).clone();
        app.begin_clipboard(ClipboardAction::Paste).unwrap();
        app.modifiers_changed(if change == 2 {
            ModifiersState::SHIFT
        } else {
            ModifiersState::empty()
        });
        app.keyboard_input(
            PhysicalKey::Code(if change == 0 {
                KeyCode::KeyQ
            } else {
                KeyCode::ArrowLeft
            }),
            &Key::Character("changed".into()),
            (change == 0).then_some("changed"),
            false,
        );
        assert!(app.pending_clipboard.is_none());
        *app.text_editor_mut(TextField::Profile).unwrap() = baseline.clone();
        app.invalidate_hits();
        drain(&mut app);
        assert_eq!(editor(&app), &baseline);
        assert!(error(&app).is_none());
    }
}

#[test]
fn field_screen_and_search_focus_round_trips_never_revive_cancelled_publication() {
    for change in 0..3 {
        let mut app = field_app(if change == 2 { 0 } else { 1 });
        install(&mut app, Ok("obsolete".into()), Ok(()));
        selected(&mut app, "base");
        let baseline = editor(&app).clone();
        app.begin_clipboard(ClipboardAction::Paste).unwrap();
        match change {
            0 => {
                app.settings.as_mut().unwrap().profile_focused = true;
                app.sync_ime();
                assert!(app.pending_clipboard.is_none());
                app.settings.as_mut().unwrap().profile_focused = false;
                app.sync_ime();
            }
            1 => {
                app.open_display();
                assert_eq!(app.navigator.route(), ScreenRoute::Display);
                assert!(app.pending_clipboard.is_none());
                app.back();
                assert_eq!(app.navigator.route(), ScreenRoute::Settings);
            }
            _ => {
                app.set_search_focus(false);
                assert!(app.pending_clipboard.is_none());
                app.set_search_focus(true);
            }
        }
        assert!(app.clipboard_busy());
        drain(&mut app);
        assert_eq!(editor(&app), &baseline);
    }
}

#[test]
fn focus_occlusion_suspend_pending_and_ime_boundaries_cancel_pending_edits() {
    for change in 0..5 {
        let mut app = field_app(2);
        install(&mut app, Ok("obsolete".into()), Ok(()));
        selected(&mut app, "base");
        let baseline = editor(&app).clone();
        app.begin_clipboard(ClipboardAction::Paste).unwrap();
        match change {
            0 => app.active = false,
            1 => app.occluded = true,
            2 => app.navigator.suspend(),
            3 => {
                let (owner, permit) = app.metadata_scope().unwrap();
                app.profile_io = Some(ProfileOperation {
                    owner,
                    permit,
                    worker: None,
                });
            }
            _ => {
                app.ime_event(Ime::Enabled);
                app.ime_event(Ime::Preedit("音".into(), Some((3, 3))));
            }
        }
        app.sync_ime();
        assert!(
            app.pending_clipboard.is_none(),
            "boundary {change} did not cancel"
        );
        assert!(app.clipboard_busy());
        app.active = true;
        app.occluded = false;
        app.profile_io = None;
        app.navigator.resume();
        app.ime_event(Ime::Disabled);
        app.sync_ime();
        drain(&mut app);
        assert_eq!(editor(&app), &baseline);
        assert!(app.clipboard.is_some());
        assert!(error(&app).is_none());
    }
}
