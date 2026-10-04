//! Deferred actual Desktop event/retained-view fixtures; no window or OS IME opens.
use super::*;

fn fields() -> [TextField; 7] {
    [
        TextField::Display(0),
        TextField::Display(1),
        TextField::Display(2),
        TextField::Display(3),
        TextField::PracticeStart,
        TextField::PracticeEnd,
        TextField::RecordDirectory,
    ]
}
fn prepared(field: TextField) -> Desktop {
    let mut app = super::tests::lifecycle_fixture();
    app.open_settings();
    match field {
        TextField::Display(index) => {
            app.open_display();
            app.display.as_mut().unwrap().selected = index;
        }
        TextField::PracticeStart | TextField::PracticeEnd => {
            app.open_practice();
            app.practice.as_mut().unwrap().end_focused = field == TextField::PracticeEnd;
        }
        TextField::RecordDirectory => app.open_records(),
        _ => unreachable!(),
    }
    app.sync_ime();
    assert_eq!(app.text_target().unwrap().field, field);
    app.text_editor_mut(field).unwrap().select_all();
    app.keyboard_input(
        PhysicalKey::Code(KeyCode::KeyX),
        &Key::Character("a別b".into()),
        Some("a別b"),
        false,
    );
    let editor = app.text_editor_mut(field).unwrap();
    editor.home();
    editor.right();
    editor.move_right(true);
    assert_eq!(editor.selection(), Some((1, 4)));
    app.sync_ime();
    app
}
fn field_error(app: &Desktop, field: TextField) -> bool {
    match field {
        TextField::Display(_) => app.display.as_ref().unwrap().error.is_some(),
        TextField::PracticeStart | TextField::PracticeEnd => {
            app.practice.as_ref().unwrap().error.is_some()
        }
        TextField::RecordDirectory => app.records.as_ref().unwrap().error.is_some(),
        _ => unreachable!(),
    }
}
fn rendered_preview(app: &mut Desktop, field: TextField, expected: &LineEditor) {
    app.draw().unwrap();
    app.scene.status().unwrap();
    // Re-submit the independently expected editor to the same retained model.
    // A missed Desktop preview route would now dirty the already composed view.
    match field {
        TextField::Display(index) => {
            let draft = app.display.as_ref().unwrap();
            let mut editors = draft.editors.clone();
            editors[index] = expected.clone();
            let view = app.display_view.as_ref().unwrap();
            assert!(!view.dirty());
            view.update(DisplayFrame {
                editors: &editors,
                selected: index,
                error: None,
                pending: false,
                hovered: None,
                armed: None,
            })
            .unwrap();
            assert!(
                !view.dirty(),
                "Display draw must already have adopted the matching preview"
            );
        }
        TextField::PracticeStart | TextField::PracticeEnd => {
            let draft = app.practice.as_ref().unwrap();
            assert!(!draft.view.dirty());
            draft.view.update(PracticeFrame {
                editor: if field == TextField::PracticeStart {
                    expected.clone()
                } else {
                    draft.editor.clone()
                },
                end_editor: if field == TextField::PracticeEnd {
                    expected.clone()
                } else {
                    draft.end_editor.clone()
                },
                end_focused: field == TextField::PracticeEnd,
                error: None,
                hovered: None,
                armed: None,
            });
            assert!(
                !draft.view.dirty(),
                "Practice draw must already have adopted the matching preview"
            );
        }
        TextField::RecordDirectory => {
            let draft = app.records.as_ref().unwrap();
            let mut frame = records_frame(
                draft,
                false,
                0,
                Some(&app.settings.as_ref().unwrap().values),
            );
            frame.directory = expected;
            let view = app.records_view.as_ref().unwrap();
            assert!(!view.dirty());
            view.update(frame).unwrap();
            assert!(
                !view.dirty(),
                "Records draw must already have adopted the matching preview"
            );
        }
        _ => unreachable!(),
    }
    assert!(app.window.is_none() && app.renderer.is_none() && app.game.is_none());
}

#[test]
fn every_new_ime_target_previews_and_paints_exact_native_ranges_then_cancels_or_commits_once() {
    for field in fields() {
        let mut app = prepared(field);
        let original = app.text_editor(field).unwrap().clone();
        let native = app.settings.as_ref().unwrap().values.native_args();
        let presentation = app.settings.as_ref().unwrap().presentation;
        app.ime_event(Ime::Commit("ignored".into()));
        assert_eq!(app.text_editor(field).unwrap(), &original);
        app.ime_event(Ime::Enabled);
        assert_eq!(app.ime.target, app.text_target());
        for end in [1, 3] {
            app.ime_event(Ime::Preedit("e\u{301}".into(), Some((1, end))));
            let expected = original.preedit("e\u{301}", Some((1, end))).unwrap();
            let shown = app.ime_editor(field, app.text_editor(field).unwrap());
            assert_eq!(shown, &expected);
            assert_eq!((shown.value(), shown.cursor()), ("ae\u{301}b", 2));
            assert_eq!(shown.composition().unwrap().range, (1, 4));
            assert_eq!(shown.composition().unwrap().selection, Some((2, 1 + end)));
            assert_eq!(app.text_editor(field).unwrap(), &original);
            rendered_preview(&mut app, field, &expected);
        }
        app.ime_event(Ime::Preedit("e\u{301}".into(), None));
        let hidden = original.preedit("e\u{301}", None).unwrap();
        assert!(
            !app.ime_editor(field, &original)
                .visible_line(40)
                .caret_visible
        );
        rendered_preview(&mut app, field, &hidden);
        app.ime_event(Ime::Preedit("".into(), None));
        assert!(app.ime.preview.is_none());
        assert_eq!(app.text_editor(field).unwrap(), &original);
        rendered_preview(&mut app, field, &original);
        app.ime_event(Ime::Preedit("音".into(), Some((0, 3))));
        app.ime_event(Ime::Commit("音".into()));
        let committed = app.text_editor(field).unwrap().clone();
        assert_eq!(
            (
                committed.value(),
                committed.cursor(),
                committed.selection(),
                committed.composition()
            ),
            ("a音b", 4, None, None)
        );
        assert!(app.ime.preview.is_none() && !app.ime.composing);
        rendered_preview(&mut app, field, &committed);
        assert_eq!(app.settings.as_ref().unwrap().values.native_args(), native);
        assert_eq!(app.settings.as_ref().unwrap().presentation, presentation);
        assert!(app.options.native.is_empty());
    }
}

#[test]
fn composition_owns_shortcuts_and_text_while_invalid_native_ranges_controls_and_limits_keep_drafts_atomic()
 {
    for field in fields() {
        let mut app = prepared(field);
        let original = app.text_editor(field).unwrap().clone();
        let route = app.navigator.route();
        let target = app.text_target();
        app.ime_event(Ime::Enabled);
        app.ime_event(Ime::Preedit("音".into(), Some((0, 3))));
        let preview = app.ime.preview.clone();
        for modifiers in [
            ModifiersState::empty(),
            ModifiersState::SHIFT,
            if cfg!(target_os = "macos") {
                ModifiersState::SUPER
            } else {
                ModifiersState::CONTROL
            },
        ] {
            app.modifiers_changed(modifiers);
            for (key, logical, text) in [
                (KeyCode::KeyQ, "a", Some("a")),
                (
                    KeyCode::KeyV,
                    "v",
                    Some("paste must not acquire a clipboard"),
                ),
                (KeyCode::ArrowLeft, "", None),
                (KeyCode::Backspace, "", None),
                (KeyCode::Tab, "", None),
                (KeyCode::Enter, "", None),
                (KeyCode::Escape, "", None),
            ] {
                app.keyboard_input(
                    PhysicalKey::Code(key),
                    &Key::Character(logical.into()),
                    text,
                    false,
                );
            }
        }
        assert_eq!(app.text_editor(field).unwrap(), &original);
        assert_eq!(app.ime.preview, preview);
        assert_eq!(app.navigator.route(), route);
        assert_eq!(app.text_target(), target);
        assert!(app.clipboard.is_none() && app.profile_io.is_none());
        let limit = match field {
            TextField::Display(_) => 32,
            TextField::RecordDirectory => 4096,
            _ => 64,
        };
        for invalid in [
            "\n".to_owned(),
            "\u{2028}".to_owned(),
            "x".repeat(limit + 1),
        ] {
            app.ime_event(Ime::Preedit(invalid.clone(), None));
            assert!(app.ime.preview.is_none());
            assert!(field_error(&app, field));
            assert_eq!(app.text_editor(field).unwrap(), &original);
            app.ime_event(Ime::Commit(invalid));
            assert!(field_error(&app, field));
            assert_eq!(app.text_editor(field).unwrap(), &original);
        }
        app.ime_event(Ime::Preedit("音".into(), Some((1, 3))));
        assert!(app.ime.preview.is_none());
        assert!(field_error(&app, field));
        assert_eq!(app.text_editor(field).unwrap(), &original);
        app.ime_event(Ime::Disabled);
        app.ime_event(Ime::Commit("late".into()));
        assert_eq!(app.text_editor(field).unwrap(), &original);
        assert!(!app.ime.enabled && !app.ime_owns_keyboard());
    }
}

#[test]
fn field_screen_and_ui_admission_changes_drop_composition_and_require_fresh_enable_for_the_current_target()
 {
    for field in fields() {
        let mut app = prepared(field);
        let original = app.text_editor(field).unwrap().clone();
        app.ime_event(Ime::Enabled);
        app.ime_event(Ime::Preedit("old".into(), None));
        match field {
            TextField::Display(index) => app.display.as_mut().unwrap().selected = (index + 1) % 4,
            TextField::PracticeStart | TextField::PracticeEnd => {
                let draft = app.practice.as_mut().unwrap();
                draft.end_focused = !draft.end_focused;
            }
            TextField::RecordDirectory => app.records.as_mut().unwrap().directory_focused = false,
            _ => unreachable!(),
        }
        app.sync_ime();
        assert!(app.ime.preview.is_none() && !app.ime.enabled);
        let new_target = app.text_target();
        let other = new_target.map(|target| app.text_editor(target.field).unwrap().clone());
        app.ime_event(Ime::Commit("stale".into()));
        assert_eq!(app.text_editor(field).unwrap(), &original);
        if let Some(target) = new_target {
            assert_eq!(app.text_editor(target.field), other.as_ref());
        }
        app.back();
        let settings = app.settings.as_ref().unwrap().editor.clone();
        app.ime_event(Ime::Commit("obsolete screen".into()));
        assert_eq!(app.settings.as_ref().unwrap().editor, settings);

        for unavailable in 0..4 {
            let mut app = prepared(field);
            let retained = app.text_editor(field).unwrap().clone();
            app.ime_event(Ime::Enabled);
            app.ime_event(Ime::Preedit("old".into(), None));
            match unavailable {
                0 => app.active = false,
                1 => app.occluded = true,
                2 => app.navigator.suspend(),
                _ => app.request_close(),
            }
            app.sync_ime();
            app.ime_event(Ime::Enabled);
            app.ime_event(Ime::Commit("stale".into()));
            assert!(app.ime.target.is_none() && app.ime.preview.is_none() && !app.ime.enabled);
            if unavailable != 3 {
                assert_eq!(app.text_editor(field).unwrap(), &retained);
                app.active = true;
                app.occluded = false;
                app.navigator.resume();
                app.sync_ime();
                app.ime_event(Ime::Preedit("not enabled".into(), None));
                assert!(app.ime.preview.is_none());
                app.ime_event(Ime::Enabled);
                app.ime_event(Ime::Commit("音".into()));
                assert_eq!(app.text_editor(field).unwrap().value(), "a音b");
            }
            assert!(app.game.is_none());
        }
    }
    let mut app = prepared(TextField::RecordDirectory);
    let original = app.records.as_ref().unwrap().directory.clone();
    app.ime_event(Ime::Enabled);
    app.ime_event(Ime::Preedit("pending".into(), None));
    let (owner, permit) = app.metadata_scope().unwrap();
    app.profile_io = Some(ProfileOperation {
        owner,
        permit,
        worker: None,
    });
    app.sync_ime();
    app.ime_event(Ime::Commit("busy".into()));
    assert!(app.ime.target.is_none() && !app.ime.enabled && app.ime.preview.is_none());
    assert_eq!(app.records.as_ref().unwrap().directory, original);
    app.profile_io = None;
    app.sync_ime();
    app.ime_event(Ime::Commit("still not enabled".into()));
    assert_eq!(app.records.as_ref().unwrap().directory, original);
}

#[test]
fn actual_font_preparation_admits_only_preview_text_without_committing_or_acquiring_native_resources()
 {
    for field in fields() {
        let mut app = prepared(field);
        app.title_font = Some(Arc::new(
            FontAtlas::new(font_fixture::font_bytes(), 14.0, 128, 128, 128).unwrap(),
        ));
        let original = app.text_editor(field).unwrap().clone();
        app.ime_event(Ime::Enabled);
        app.ime_event(Ime::Preedit("가\u{301}音".into(), Some((3, 5))));
        app.prepare_input_font().unwrap();
        let retained = Arc::clone(app.title_font.as_ref().unwrap());
        for character in ['가', '\u{301}', '音'] {
            assert!(retained.get(character).is_some());
        }
        assert_eq!(app.text_editor(field).unwrap(), &original);
        assert!(app.input_font_error.is_none());
        app.ime_event(Ime::Preedit("".into(), None));
        app.prepare_input_font().unwrap();
        assert!(Arc::ptr_eq(&retained, app.title_font.as_ref().unwrap()));
        assert!(
            app.renderer.is_none()
                && app.window.is_none()
                && app.game.is_none()
                && app.profile_io.is_none()
        );
    }
    let mut old_routes = super::tests::lifecycle_fixture();
    old_routes.set_search_focus(true);
    old_routes.ime_event(Ime::Enabled);
    old_routes.ime_event(Ime::Preedit("音".into(), None));
    assert_eq!(old_routes.search_editor.value(), "");
    old_routes.ime_event(Ime::Commit("音".into()));
    assert_eq!(old_routes.search_editor.value(), "音");
    old_routes.open_settings();
    old_routes.settings.as_mut().unwrap().profile_focused = true;
    old_routes.sync_ime();
    old_routes.ime_event(Ime::Enabled);
    old_routes.ime_event(Ime::Commit("音.json".into()));
    assert_eq!(
        old_routes.settings.as_ref().unwrap().profile.value(),
        "音.json"
    );
}

#[test]
fn record_directory_commit_invalidates_old_catalog_and_preview_only_when_the_committed_value_changes()
 {
    let mut app = prepared(TextField::RecordDirectory);
    let path = PathBuf::from("old/record.bkr");
    {
        let records = app.records.as_mut().unwrap();
        records.catalog = Some(RecordCatalog {
            entries: vec![path.clone()],
            truncated: false,
        });
        records.selected = Some(0);
        records.first = 10;
        records.message = Some("retained metadata".into());
        records.preview = Some(RecordPreview {
            path: path.clone(),
            records: 7,
            recorded_until: Some(beatkernel::time::Timestamp::from_nanos(123)),
            start: beatkernel::time::Timestamp::ZERO,
            score: Default::default(),
        });
    }
    let original = app.records.as_ref().unwrap().directory.clone();
    app.ime_event(Ime::Enabled);
    app.ime_event(Ime::Preedit("新".into(), None));
    app.ime_event(Ime::Preedit("".into(), None));
    assert_eq!(app.records.as_ref().unwrap().directory, original);
    assert_eq!(
        app.records.as_ref().unwrap().valid_preview().unwrap().path,
        path
    );
    app.records.as_mut().unwrap().directory.clear_selection();
    app.ime_event(Ime::Commit("".into()));
    assert_eq!(
        app.records
            .as_ref()
            .unwrap()
            .catalog
            .as_ref()
            .unwrap()
            .entries,
        [path.clone()]
    );
    assert_eq!(
        app.records
            .as_ref()
            .unwrap()
            .valid_preview()
            .unwrap()
            .records,
        7
    );
    app.ime_event(Ime::Preedit("新".into(), Some((0, 3))));
    app.ime_event(Ime::Commit("新".into()));
    let records = app.records.as_ref().unwrap();
    assert_eq!(records.directory.value(), "a別新b");
    assert!(records.catalog.is_none() && records.preview.is_none() && records.selected.is_none());
    assert_eq!(records.first, 0);
    assert!(records.message.is_none());
    assert!(app.profile_io.is_none() && app.game.is_none() && app.clipboard.is_none());
}
